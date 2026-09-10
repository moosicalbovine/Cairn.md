use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::import::{
    absolute_path_key, collision_name, is_same_or_descendant, validate_tracked_relative_path,
    ImportSource, TrackedFolderEntry, TrackedFolderSnapshot,
};
use crate::domain::project::{
    document_relative_path, path_key, validate_document_name, validate_project_name,
    validate_relative_path,
};
use crate::domain::recovery::{
    validate_snapshot_request, RecoveryLifecycle, RecoverySnapshot, RecoverySnapshotRequest,
    SaveDocumentResult, SaveStatus,
};
use crate::infrastructure::atomic_write::replace_if_unchanged;
use crate::infrastructure::copy::{
    copy_stable_source, inspect_markdown_source, remove_owned_file, SourceDescriptor,
};
use crate::infrastructure::database::{Database, DatabaseOpen};
use crate::infrastructure::filesystem::{
    case_aware_rename, copy_durable_no_replace, file_identity, fingerprint, probe_candidate,
    recycle_file, rename_no_replace, resolve_existing, resolve_new, scan, sync_parent,
    write_durable, CandidateRootProbe, ScannedProject,
};
use crate::infrastructure::watcher::{LibraryWatcher, WatcherHints};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryError {
    code: String,
    message: String,
}

impl LibraryError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn invalid_path(message: impl Into<String>) -> Self {
        Self::new("invalid_path", message)
    }

    pub fn path_escape(message: impl Into<String>) -> Self {
        Self::new("path_escape", message)
    }

    pub fn io(error: std::io::Error) -> Self {
        Self::new("io_error", error.to_string())
    }

    pub fn database(error: rusqlite::Error) -> Self {
        Self::new("metadata_error", error.to_string())
    }

    pub fn watcher(error: notify::Error) -> Self {
        Self::new("watcher_error", error.to_string())
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for LibraryError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryMode {
    Writable,
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBinding {
    pub library_id: String,
    pub root_path: PathBuf,
    pub root_identity: String,
    pub generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSnapshot {
    pub id: String,
    pub relative_path: String,
    pub source_path: Option<String>,
    pub imported_at: Option<i64>,
    pub disk_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentContent {
    pub document: DocumentSnapshot,
    pub bytes: Vec<u8>,
    pub base_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub id: String,
    pub relative_path: String,
    pub documents: Vec<DocumentSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub mode: LibraryMode,
    pub read_only_reason: Option<String>,
    pub binding: Option<LibraryBinding>,
    pub projects: Vec<ProjectSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkPreview {
    pub candidate_path: PathBuf,
    pub library_id: String,
    pub expected_generation: i64,
    pub expected_state_token: String,
    pub root_identity: String,
    pub matched_projects: usize,
    pub matched_documents: usize,
    pub candidate_manifest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedDocument {
    pub recovery_path: Option<PathBuf>,
    pub recycled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalPhase {
    #[serde(rename = "Intent recorded")]
    IntentRecorded,
    #[serde(rename = "Temporary durable")]
    TemporaryDurable,
    #[serde(rename = "Filesystem finalized")]
    FilesystemFinalized,
    #[serde(rename = "Metadata committed")]
    MetadataCommitted,
    #[serde(rename = "Cleanup complete")]
    CleanupComplete,
}

impl JournalPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::IntentRecorded => "Intent recorded",
            Self::TemporaryDurable => "Temporary durable",
            Self::FilesystemFinalized => "Filesystem finalized",
            Self::MetadataCommitted => "Metadata committed",
            Self::CleanupComplete => "Cleanup complete",
        }
    }

    fn parse(value: &str) -> Result<Self, LibraryError> {
        match value {
            "Intent recorded" => Ok(Self::IntentRecorded),
            "Temporary durable" => Ok(Self::TemporaryDurable),
            "Filesystem finalized" => Ok(Self::FilesystemFinalized),
            "Metadata committed" => Ok(Self::MetadataCommitted),
            "Cleanup complete" => Ok(Self::CleanupComplete),
            _ => Err(LibraryError::new(
                "journal_invalid",
                "Unknown journal phase",
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OperationPayload {
    CreateProject {
        project_id: String,
        relative_path: String,
    },
    RenameProject {
        project_id: String,
        from_relative_path: String,
        to_relative_path: String,
    },
    CreateDocument {
        document_id: String,
        project_id: String,
        relative_path: String,
    },
    MoveDocument {
        document_id: String,
        target_project_id: String,
        from_relative_path: String,
        to_relative_path: String,
    },
    DeleteDocument {
        document_id: String,
        relative_path: String,
        #[serde(default)]
        recovery_relative_path: Option<String>,
    },
    ImportDocument {
        document_id: String,
        project_id: String,
        relative_path: String,
        source_path: String,
        source_resolved_path: PathBuf,
        source_identity: String,
        source_fingerprint: String,
        imported_at: i64,
    },
    SaveDocument {
        document_id: String,
        relative_path: String,
        session_generation: String,
        revision: i64,
        intended_fingerprint: String,
        backup_relative_path: String,
    },
}

pub struct LibraryService {
    database: Option<Database>,
    mode: LibraryMode,
    read_only_reason: Option<String>,
    watcher_hints: WatcherHints,
    watcher: Option<LibraryWatcher>,
}

impl LibraryService {
    pub fn open(app_data_dir: &Path) -> Result<Self, LibraryError> {
        let app_data_dir = app_data_dir.to_path_buf();
        let (database, mode, read_only_reason) = match Database::open(&app_data_dir)? {
            DatabaseOpen::Healthy(database) => (Some(database), LibraryMode::Writable, None),
            DatabaseOpen::Damaged { reason, .. } => {
                log::error!("Cairn.md metadata unavailable: {reason}");
                (
                    None,
                    LibraryMode::ReadOnly,
                    Some("metadata_damaged".to_owned()),
                )
            }
        };
        let mut service = Self {
            database,
            mode,
            read_only_reason,
            watcher_hints: WatcherHints::default(),
            watcher: None,
        };

        if service.database.is_some() {
            match service.binding() {
                Ok(Some(binding)) => match file_identity(&binding.root_path) {
                    Ok(Some(identity)) if identity == binding.root_identity => {
                        match probe_candidate(&binding.root_path) {
                            Ok(probe)
                                if probe.can_bind
                                    && probe.root_identity.as_deref()
                                        == Some(binding.root_identity.as_str()) =>
                            {
                                if let Err(error) = service.replay_pending_operations() {
                                    log::error!("Cairn.md journal replay failed: {error}");
                                    service.enter_read_only("journal_recovery_failed");
                                    service.preserve_metadata_evidence();
                                    return Ok(service);
                                }
                                if let Err(error) = service.restart_watcher(&binding.root_path) {
                                    log::error!("Cairn.md watcher startup failed: {error}");
                                    service.enter_read_only("watcher_unavailable");
                                    service.preserve_metadata_evidence();
                                }
                            }
                            Ok(probe) if probe.can_bind => {
                                service.enter_read_only("root_identity_mismatch")
                            }
                            Ok(_) | Err(_) => service.enter_read_only("root_capability_lost"),
                        }
                    }
                    Ok(_) => service.enter_read_only("root_identity_mismatch"),
                    Err(_) => service.enter_read_only("root_capability_lost"),
                },
                Ok(None) => {}
                Err(error) => {
                    log::error!("Cairn.md binding read failed: {error}");
                    service.enter_read_only("metadata_damaged");
                    service.preserve_metadata_evidence();
                    service.database = None;
                }
            }
        }
        Ok(service)
    }

    pub fn probe_candidate(&self, candidate: &Path) -> Result<CandidateRootProbe, LibraryError> {
        probe_candidate(candidate)
    }

    pub fn bind_root(&mut self, root: &Path) -> Result<LibrarySnapshot, LibraryError> {
        self.require_metadata()?;
        if self.binding()?.is_some() {
            return Err(LibraryError::new(
                "library_already_bound",
                "Use the explicit relink flow to replace the active root",
            ));
        }
        let probe = probe_candidate(root)?;
        if !probe.can_bind {
            return Err(probe_error(&probe));
        }
        let root_identity = probe
            .root_identity
            .clone()
            .ok_or_else(|| LibraryError::new("root_unavailable", "Root identity unavailable"))?;
        let root_path = probe.candidate_path;
        let library_id = Uuid::new_v4().to_string();
        let scanned = scan(&root_path)?;
        let manifest = inventory_manifest(&scanned);
        let watcher = LibraryWatcher::start(&root_path, self.watcher_hints.clone())?;
        let now = now_millis();
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "INSERT INTO libraries (id, root_path, root_identity, binding_generation, previous_root_path, manifest_json, updated_at) VALUES (?1, ?2, ?3, 1, NULL, ?4, ?5)",
            params![&library_id, path_text(&root_path), &root_identity, &manifest, now],
        ).map_err(LibraryError::database)?;
        reconcile_transaction(&transaction, &library_id, &scanned, &manifest, now, false)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.watcher = Some(watcher);
        self.mode = LibraryMode::Writable;
        self.read_only_reason = None;
        self.snapshot()
    }

    pub fn preview_relink(&self, candidate: &Path) -> Result<RelinkPreview, LibraryError> {
        self.require_metadata()?;
        let binding = self
            .binding()?
            .ok_or_else(|| LibraryError::new("library_not_bound", "No library is bound"))?;
        let probe = probe_candidate(candidate)?;
        if !probe.can_bind {
            return Err(probe_error(&probe));
        }
        let scanned = scan(&probe.candidate_path)?;
        let snapshot = self.snapshot()?;
        let matched_projects = scanned
            .iter()
            .filter(|candidate| {
                snapshot.projects.iter().any(|project| {
                    path_key(&project.relative_path) == path_key(&candidate.relative_path)
                })
            })
            .count();
        let matched_documents = scanned
            .iter()
            .flat_map(|project| &project.documents)
            .filter(|candidate| {
                snapshot
                    .projects
                    .iter()
                    .flat_map(|project| &project.documents)
                    .any(|document| {
                        path_key(&document.relative_path) == path_key(&candidate.relative_path)
                    })
            })
            .count();
        let candidate_manifest = inventory_manifest(&scanned);
        Ok(RelinkPreview {
            candidate_path: probe.candidate_path,
            library_id: binding.library_id,
            expected_generation: binding.generation,
            expected_state_token: relink_state_token(&snapshot, &candidate_manifest),
            root_identity: probe.root_identity.unwrap_or_default(),
            matched_projects,
            matched_documents,
            candidate_manifest,
        })
    }

    pub fn confirm_relink(
        &mut self,
        preview: RelinkPreview,
    ) -> Result<LibrarySnapshot, LibraryError> {
        self.require_metadata()?;
        let binding = self
            .binding()?
            .ok_or_else(|| LibraryError::new("library_not_bound", "No library is bound"))?;
        if binding.library_id != preview.library_id
            || binding.generation != preview.expected_generation
            || relink_state_token(&self.snapshot()?, &preview.candidate_manifest)
                != preview.expected_state_token
        {
            return Err(LibraryError::new(
                "stale_relink",
                "The library changed after the relink preview",
            ));
        }
        let fresh = self.preview_relink(&preview.candidate_path)?;
        if fresh.root_identity != preview.root_identity
            || fresh.expected_state_token != preview.expected_state_token
            || fresh.candidate_manifest != preview.candidate_manifest
        {
            return Err(LibraryError::new(
                "stale_relink",
                "The candidate changed after the relink preview",
            ));
        }
        let scanned = scan(&preview.candidate_path)?;
        let manifest = inventory_manifest(&scanned);
        let watcher = LibraryWatcher::start(&preview.candidate_path, self.watcher_hints.clone())?;
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "UPDATE libraries SET previous_root_path = root_path, root_path = ?1, root_identity = ?2, binding_generation = binding_generation + 1, manifest_json = ?3, updated_at = ?4 WHERE id = ?5",
            params![path_text(&preview.candidate_path), &preview.root_identity, &manifest, now_millis(), &preview.library_id],
        ).map_err(LibraryError::database)?;
        reconcile_transaction(
            &transaction,
            &preview.library_id,
            &scanned,
            &manifest,
            now_millis(),
            true,
        )?;
        transaction.commit().map_err(LibraryError::database)?;
        self.watcher = Some(watcher);
        self.mode = LibraryMode::Writable;
        self.read_only_reason = None;
        self.snapshot()
    }

    pub fn snapshot(&self) -> Result<LibrarySnapshot, LibraryError> {
        let Some(database) = &self.database else {
            return Ok(LibrarySnapshot {
                mode: LibraryMode::ReadOnly,
                read_only_reason: self.read_only_reason.clone(),
                binding: None,
                projects: Vec::new(),
            });
        };
        let binding = query_binding(database)?;
        let Some(active) = &binding else {
            return Ok(LibrarySnapshot {
                mode: self.mode,
                read_only_reason: self.read_only_reason.clone(),
                binding: None,
                projects: Vec::new(),
            });
        };
        let mut project_statement = database
            .connection()
            .prepare(
                "SELECT id, relative_path FROM projects WHERE library_id = ?1 ORDER BY path_key",
            )
            .map_err(LibraryError::database)?;
        let project_rows = project_statement
            .query_map([&active.library_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(LibraryError::database)?;
        let mut projects = Vec::new();
        for row in project_rows {
            let (id, relative_path) = row.map_err(LibraryError::database)?;
            let mut document_statement = database.connection().prepare(
                "SELECT id, relative_path, source_path, imported_at, disk_fingerprint FROM documents WHERE project_id = ?1 ORDER BY path_key",
            ).map_err(LibraryError::database)?;
            let documents = document_statement
                .query_map([&id], |document| {
                    Ok(DocumentSnapshot {
                        id: document.get(0)?,
                        relative_path: document.get(1)?,
                        source_path: document.get(2)?,
                        imported_at: document.get(3)?,
                        disk_fingerprint: document.get(4)?,
                    })
                })
                .map_err(LibraryError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(LibraryError::database)?;
            projects.push(ProjectSnapshot {
                id,
                relative_path,
                documents,
            });
        }
        Ok(LibrarySnapshot {
            mode: self.mode,
            read_only_reason: self.read_only_reason.clone(),
            binding,
            projects,
        })
    }

    pub fn create_project(&mut self, name: &str) -> Result<ProjectSnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        let name = validate_project_name(name)?;
        self.ensure_project_path_available(&binding.library_id, &name, None)?;
        let target = resolve_new(&binding.root_path, &name)?;
        let id = Uuid::new_v4().to_string();
        let operation_id = Uuid::new_v4().to_string();
        let payload = OperationPayload::CreateProject {
            project_id: id.clone(),
            relative_path: name.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "create_project",
            &payload,
            None,
            None,
        )?;
        self.verify_active_root(&binding)?;
        fs::create_dir(&target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        let identity = file_identity(&target)?;
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            None,
            identity.as_deref(),
        )?;
        self.commit_project_metadata(
            &operation_id,
            &id,
            &binding.library_id,
            &name,
            identity.as_deref(),
        )?;
        self.finish_operation(&operation_id)?;
        self.project_by_id(&id)
    }

    pub fn rename_project(
        &mut self,
        project_id: &str,
        name: &str,
    ) -> Result<ProjectSnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        let name = validate_project_name(name)?;
        let project = self.project_by_id(project_id)?;
        self.ensure_project_path_available(&binding.library_id, &name, Some(project_id))?;
        let source = self.verified_project_path(&binding, project_id, &project.relative_path)?;
        validate_relative_path(&project.relative_path, 1)?;
        let target = resolve_new(&binding.root_path, &name)?;
        let operation_id = Uuid::new_v4().to_string();
        let payload = OperationPayload::RenameProject {
            project_id: project_id.to_owned(),
            from_relative_path: project.relative_path.clone(),
            to_relative_path: name.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "rename_project",
            &payload,
            None,
            None,
        )?;
        self.verify_active_root(&binding)?;
        self.verify_project_file(&source, project_id)?;
        case_aware_rename(&source, &target, &operation_id).map_err(map_conflict_io)?;
        let identity = file_identity(&target)?;
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            None,
            identity.as_deref(),
        )?;
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "UPDATE projects SET relative_path = ?1, path_key = ?2, file_identity = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, path_key(&name), identity, now_millis(), project_id],
        ).map_err(database_conflict)?;
        let mut statement = transaction
            .prepare("SELECT id, relative_path FROM documents WHERE project_id = ?1")
            .map_err(LibraryError::database)?;
        let paths = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(LibraryError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(LibraryError::database)?;
        drop(statement);
        for (document_id, old_path) in paths {
            let file_name = Path::new(&old_path)
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| LibraryError::invalid_path("Document path is invalid"))?;
            let relative = document_relative_path(&name, file_name);
            transaction.execute(
                "UPDATE documents SET relative_path = ?1, path_key = ?2, updated_at = ?3 WHERE id = ?4",
                params![relative, path_key(&relative), now_millis(), document_id],
            ).map_err(database_conflict)?;
        }
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now_millis(),
                    &operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.finish_operation(&operation_id)?;
        self.project_by_id(project_id)
    }

    pub fn create_document(
        &mut self,
        project_id: &str,
        name: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        self.create_document_through_phase(project_id, name, JournalPhase::CleanupComplete, false)?
            .ok_or_else(|| LibraryError::new("journal_incomplete", "Document was not committed"))
    }

    pub fn read_document(&mut self, document_id: &str) -> Result<DocumentContent, LibraryError> {
        let binding = self.required_binding()?;
        self.verify_active_root(&binding)?;
        let document = self.document_by_id(document_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        let path = self.verified_document_path(&binding, document_id, &document.relative_path)?;
        let bytes = fs::read(&path).map_err(LibraryError::io)?;
        let base_fingerprint = format!("sha256:{:x}", Sha256::digest(&bytes));
        Ok(DocumentContent {
            document,
            bytes,
            base_fingerprint,
        })
    }

    pub fn store_recovery_snapshot(
        &mut self,
        request: RecoverySnapshotRequest,
    ) -> Result<RecoverySnapshot, LibraryError> {
        self.document_by_id(&request.document_id)?;
        let intended_disk_hash = validate_snapshot_request(&request)?;
        let durable_at = now_millis();
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let existing = transaction
            .query_row(
                "SELECT session_generation, revision, lifecycle_state, content_hash, base_fingerprint FROM recovery_snapshots WHERE document_id = ?1",
                [&request.document_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(LibraryError::database)?;

        if let Some((generation, revision, lifecycle, content_hash, base_fingerprint)) = existing {
            if generation != request.session_generation {
                return Err(LibraryError::new(
                    "recovery_pending",
                    "A recovery snapshot from another editing session must be resolved first",
                ));
            }
            if RecoveryLifecycle::parse(&lifecycle)? == RecoveryLifecycle::Conflict {
                return Err(LibraryError::new(
                    "external_conflict",
                    "The external-change conflict must be resolved before editing continues",
                ));
            }
            if revision > request.revision {
                transaction.commit().map_err(LibraryError::database)?;
                return self
                    .load_recovery_snapshot(&request.document_id)?
                    .ok_or_else(|| {
                        LibraryError::new(
                            "recovery_missing",
                            "Recovery snapshot disappeared while it was being read",
                        )
                    });
            }
            if revision == request.revision {
                if content_hash != intended_disk_hash {
                    return Err(LibraryError::new(
                        "recovery_revision_collision",
                        "One recovery revision cannot represent different document bytes",
                    ));
                }
                if base_fingerprint == request.base_fingerprint
                    || RecoveryLifecycle::parse(&lifecycle)? != RecoveryLifecycle::Draft
                {
                    transaction.commit().map_err(LibraryError::database)?;
                    return self
                        .load_recovery_snapshot(&request.document_id)?
                        .ok_or_else(|| {
                            LibraryError::new(
                                "recovery_missing",
                                "Recovery snapshot disappeared while it was being read",
                            )
                        });
                }
            }
        }

        transaction
            .execute(
                "INSERT INTO recovery_snapshots (document_id, session_generation, revision, content, content_hash, base_fingerprint, intended_disk_hash, operation_id, lifecycle_state, durable_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, NULL, ?7, ?8) ON CONFLICT(document_id) DO UPDATE SET revision = excluded.revision, content = excluded.content, content_hash = excluded.content_hash, base_fingerprint = excluded.base_fingerprint, intended_disk_hash = excluded.intended_disk_hash, operation_id = NULL, lifecycle_state = excluded.lifecycle_state, durable_at = excluded.durable_at",
                params![
                    &request.document_id,
                    &request.session_generation,
                    request.revision,
                    &request.bytes,
                    &intended_disk_hash,
                    &request.base_fingerprint,
                    RecoveryLifecycle::Draft.as_str(),
                    durable_at,
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.load_recovery_snapshot(&request.document_id)?
            .ok_or_else(|| {
                LibraryError::new("recovery_missing", "Recovery snapshot was not stored")
            })
    }

    pub fn load_recovery_snapshot(
        &self,
        document_id: &str,
    ) -> Result<Option<RecoverySnapshot>, LibraryError> {
        self.document_by_id(document_id)?;
        query_recovery_snapshot(self.require_metadata()?, document_id)
    }

    pub fn discard_recovery_snapshot(
        &mut self,
        document_id: &str,
        session_generation: &str,
    ) -> Result<bool, LibraryError> {
        self.document_by_id(document_id)?;
        if Uuid::parse_str(session_generation).is_err() {
            return Err(LibraryError::new(
                "recovery_invalid",
                "Recovery session generation is invalid",
            ));
        }
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let removed = transaction
            .execute(
                "DELETE FROM recovery_snapshots WHERE document_id = ?1 AND session_generation = ?2",
                params![document_id, session_generation],
            )
            .map_err(LibraryError::database)?;
        if removed == 1 {
            transaction
                .execute(
                    "DELETE FROM external_conflicts WHERE document_id = ?1",
                    [document_id],
                )
                .map_err(LibraryError::database)?;
        }
        transaction.commit().map_err(LibraryError::database)?;
        Ok(removed == 1)
    }

    pub fn save_document(
        &mut self,
        request: RecoverySnapshotRequest,
    ) -> Result<SaveDocumentResult, LibraryError> {
        let intended_fingerprint = validate_snapshot_request(&request)?;
        let snapshot = self
            .load_recovery_snapshot(&request.document_id)?
            .ok_or_else(|| {
                LibraryError::new(
                    "recovery_missing",
                    "A durable recovery snapshot is required before saving",
                )
            })?;
        if snapshot.session_generation != request.session_generation
            || snapshot.revision != request.revision
            || snapshot.bytes != request.bytes
            || snapshot.base_fingerprint != request.base_fingerprint
            || snapshot.intended_disk_hash != intended_fingerprint
        {
            return Err(LibraryError::new(
                "recovery_mismatch",
                "The save request does not match its durable recovery snapshot",
            ));
        }

        let binding = self.required_writable_binding()?;
        let document = self.document_by_id(&request.document_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        let project = self.project_for_document(&request.document_id)?;
        self.verified_project_path(&binding, &project.id, &project.relative_path)?;
        let operation_id = Uuid::new_v4().to_string();
        let temporary_relative = document_relative_path(
            &project.relative_path,
            &format!(".cairn-save-{operation_id}.tmp"),
        );
        let backup_relative = document_relative_path(
            &project.relative_path,
            &format!(".cairn-save-{operation_id}.backup"),
        );
        let payload = OperationPayload::SaveDocument {
            document_id: request.document_id.clone(),
            relative_path: document.relative_path,
            session_generation: request.session_generation.clone(),
            revision: request.revision,
            intended_fingerprint: intended_fingerprint.clone(),
            backup_relative_path: backup_relative,
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "save_document",
            &payload,
            Some(&temporary_relative),
            Some(&request.base_fingerprint),
        )?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE recovery_snapshots SET operation_id = ?1, lifecycle_state = ?2 WHERE document_id = ?3 AND session_generation = ?4 AND revision = ?5",
            params![
                &operation_id,
                RecoveryLifecycle::Saving.as_str(),
                &request.document_id,
                &request.session_generation,
                request.revision,
            ],
        ).map_err(LibraryError::database)?;
        self.replay_pending_operations()?;

        if let Some(external_hash) = self.external_conflict_hash(&request.document_id)? {
            return Ok(SaveDocumentResult {
                status: SaveStatus::Conflict,
                revision: request.revision,
                disk_fingerprint: external_hash,
            });
        }
        let saved = self.document_by_id(&request.document_id)?;
        if saved.disk_fingerprint != intended_fingerprint {
            return Err(LibraryError::new(
                "save_verification_failed",
                "Saved metadata does not match the intended document bytes",
            ));
        }
        Ok(SaveDocumentResult {
            status: SaveStatus::Saved,
            revision: request.revision,
            disk_fingerprint: saved.disk_fingerprint,
        })
    }

    pub fn save_document_interrupted_for_test(
        &mut self,
        request: RecoverySnapshotRequest,
        stop_after: JournalPhase,
    ) -> Result<(), LibraryError> {
        self.save_document_through_phase_for_test(request, stop_after, false)
    }

    pub fn save_document_interrupted_after_replace_before_phase_for_test(
        &mut self,
        request: RecoverySnapshotRequest,
    ) -> Result<(), LibraryError> {
        self.save_document_through_phase_for_test(request, JournalPhase::FilesystemFinalized, true)
    }

    fn save_document_through_phase_for_test(
        &mut self,
        request: RecoverySnapshotRequest,
        stop_after: JournalPhase,
        stop_before_finalize_record: bool,
    ) -> Result<(), LibraryError> {
        let intended_fingerprint = validate_snapshot_request(&request)?;
        let snapshot = self
            .load_recovery_snapshot(&request.document_id)?
            .ok_or_else(|| LibraryError::new("recovery_missing", "No recovery snapshot exists"))?;
        if snapshot.session_generation != request.session_generation
            || snapshot.revision != request.revision
            || snapshot.bytes != request.bytes
            || snapshot.base_fingerprint != request.base_fingerprint
            || snapshot.intended_disk_hash != intended_fingerprint
        {
            return Err(LibraryError::new(
                "recovery_mismatch",
                "The save request does not match its durable recovery snapshot",
            ));
        }
        let binding = self.required_writable_binding()?;
        let document = self.document_by_id(&request.document_id)?;
        let project = self.project_for_document(&request.document_id)?;
        self.verified_project_path(&binding, &project.id, &project.relative_path)?;
        let operation_id = Uuid::new_v4().to_string();
        let temporary_relative = document_relative_path(
            &project.relative_path,
            &format!(".cairn-save-{operation_id}.tmp"),
        );
        let backup_relative = document_relative_path(
            &project.relative_path,
            &format!(".cairn-save-{operation_id}.backup"),
        );
        let payload = OperationPayload::SaveDocument {
            document_id: request.document_id.clone(),
            relative_path: document.relative_path.clone(),
            session_generation: request.session_generation.clone(),
            revision: request.revision,
            intended_fingerprint: intended_fingerprint.clone(),
            backup_relative_path: backup_relative.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "save_document",
            &payload,
            Some(&temporary_relative),
            Some(&request.base_fingerprint),
        )?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE recovery_snapshots SET operation_id = ?1, lifecycle_state = ?2 WHERE document_id = ?3 AND session_generation = ?4 AND revision = ?5",
            params![&operation_id, RecoveryLifecycle::Saving.as_str(), &request.document_id, &request.session_generation, request.revision],
        ).map_err(LibraryError::database)?;
        if stop_after == JournalPhase::IntentRecorded {
            return Ok(());
        }

        let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
        write_durable(&temporary, &request.bytes)?;
        self.update_operation(
            &operation_id,
            JournalPhase::TemporaryDurable,
            None,
            file_identity(&temporary)?.as_deref(),
        )?;
        if stop_after == JournalPhase::TemporaryDurable {
            return Ok(());
        }

        let target = resolve_new(&binding.root_path, &document.relative_path)?;
        let backup = resolve_new(&binding.root_path, &backup_relative)?;
        let expected_identity = self.document_identity(&request.document_id)?;
        let replaced = replace_if_unchanged(
            &target,
            &temporary,
            &backup,
            &request.base_fingerprint,
            expected_identity.as_deref(),
            &intended_fingerprint,
        )?;
        if stop_before_finalize_record {
            return Ok(());
        }
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&replaced.fingerprint),
            replaced.file_identity.as_deref(),
        )?;
        if stop_after == JournalPhase::FilesystemFinalized {
            return Ok(());
        }

        self.commit_saved_document(
            &operation_id,
            &request.document_id,
            &request.session_generation,
            request.revision,
            &intended_fingerprint,
            replaced.file_identity.as_deref(),
        )?;
        if stop_after == JournalPhase::MetadataCommitted {
            return Ok(());
        }
        cleanup_owned_artifact(&backup, &request.base_fingerprint)?;
        self.finish_operation(&operation_id)
    }

    pub fn save_recovery_copy(
        &mut self,
        document_id: &str,
        session_generation: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        let snapshot = self
            .load_recovery_snapshot(document_id)?
            .ok_or_else(|| LibraryError::new("recovery_missing", "No recovery snapshot exists"))?;
        if snapshot.session_generation != session_generation {
            return Err(LibraryError::new(
                "recovery_mismatch",
                "Recovery session generation does not match",
            ));
        }
        let binding = self.required_writable_binding()?;
        let project = self.project_for_document(document_id)?;
        let document = self.document_by_id(document_id)?;
        let original_name = document
            .relative_path
            .split('/')
            .nth(1)
            .ok_or_else(|| LibraryError::invalid_path("Stored document path is invalid"))?;
        let original_path = Path::new(original_name);
        let stem = original_path
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| LibraryError::invalid_path("Stored document name is invalid"))?;
        let recovered_name = format!("{stem} (recovered).md");

        for sequence in 1..=10_000 {
            let name = collision_name(&recovered_name, sequence)?;
            let relative = document_relative_path(&project.relative_path, &name);
            if self.document_destination_exists(&binding, &relative)? {
                continue;
            }
            let recovered = self.create_document(&project.id, &name)?;
            let empty = self.read_document(&recovered.id)?;
            let request = RecoverySnapshotRequest {
                document_id: recovered.id.clone(),
                session_generation: Uuid::new_v4().to_string(),
                revision: 1,
                bytes: snapshot.bytes.clone(),
                base_fingerprint: empty.base_fingerprint,
            };
            self.store_recovery_snapshot(request.clone())?;
            let result = self.save_document(request)?;
            if result.status != SaveStatus::Saved {
                return Err(LibraryError::new(
                    "recovery_copy_conflict",
                    "The recovered copy changed before it could be saved",
                ));
            }
            self.discard_recovery_snapshot(document_id, session_generation)?;
            return self.document_by_id(&recovered.id);
        }
        Err(LibraryError::new(
            "collision_limit",
            "No recovered document name is available",
        ))
    }

    fn record_external_conflict(
        &mut self,
        operation_id: &str,
        document_id: &str,
        snapshot: &RecoverySnapshot,
        target: &Path,
    ) -> Result<(), LibraryError> {
        let (external_bytes, external_hash) = match fs::read(target) {
            Ok(bytes) => {
                let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
                (bytes, hash)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (Vec::new(), "missing".to_owned())
            }
            Err(error) => return Err(LibraryError::io(error)),
        };
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "INSERT INTO external_conflicts (document_id, operation_id, external_bytes, external_hash, draft_revision, draft_hash, captured_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(document_id) DO UPDATE SET operation_id = excluded.operation_id, external_bytes = excluded.external_bytes, external_hash = excluded.external_hash, draft_revision = excluded.draft_revision, draft_hash = excluded.draft_hash, captured_at = excluded.captured_at",
            params![
                document_id,
                operation_id,
                external_bytes,
                external_hash,
                snapshot.revision,
                &snapshot.intended_disk_hash,
                now_millis(),
            ],
        ).map_err(LibraryError::database)?;
        transaction.execute(
            "UPDATE recovery_snapshots SET operation_id = ?1, lifecycle_state = ?2 WHERE document_id = ?3 AND session_generation = ?4 AND revision = ?5",
            params![operation_id, RecoveryLifecycle::Conflict.as_str(), document_id, &snapshot.session_generation, snapshot.revision],
        ).map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)
    }

    fn commit_saved_document(
        &mut self,
        operation_id: &str,
        document_id: &str,
        session_generation: &str,
        revision: i64,
        intended_fingerprint: &str,
        target_identity: Option<&str>,
    ) -> Result<(), LibraryError> {
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let changed = transaction.execute(
            "UPDATE documents SET disk_fingerprint = ?1, disk_revision = ?2, file_identity = ?3, updated_at = ?4 WHERE id = ?5",
            params![intended_fingerprint, revision, target_identity, now_millis(), document_id],
        ).map_err(LibraryError::database)?;
        if changed != 1 {
            return Err(LibraryError::new(
                "document_not_found",
                "Saved document metadata is missing",
            ));
        }
        let cleared = transaction.execute(
            "DELETE FROM recovery_snapshots WHERE document_id = ?1 AND session_generation = ?2 AND revision = ?3 AND intended_disk_hash = ?4",
            params![document_id, session_generation, revision, intended_fingerprint],
        ).map_err(LibraryError::database)?;
        if cleared != 1 {
            return Err(LibraryError::new(
                "recovery_mismatch",
                "The durable recovery snapshot changed before metadata commit",
            ));
        }
        transaction
            .execute(
                "DELETE FROM external_conflicts WHERE document_id = ?1",
                [document_id],
            )
            .map_err(LibraryError::database)?;
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now_millis(),
                    operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)
    }

    pub fn import_document(
        &mut self,
        project_id: &str,
        source: ImportSource,
    ) -> Result<DocumentSnapshot, LibraryError> {
        self.import_document_through_phase(
            project_id,
            source,
            JournalPhase::CleanupComplete,
            false,
        )?
        .ok_or_else(|| LibraryError::new("journal_incomplete", "Import was not committed"))
    }

    pub fn import_document_interrupted_for_test(
        &mut self,
        project_id: &str,
        source: ImportSource,
        stop_after: JournalPhase,
    ) -> Result<(), LibraryError> {
        self.import_document_through_phase(project_id, source, stop_after, false)?;
        Ok(())
    }

    pub fn import_document_interrupted_after_finalize_before_phase_for_test(
        &mut self,
        project_id: &str,
        source: ImportSource,
    ) -> Result<(), LibraryError> {
        self.import_document_through_phase(
            project_id,
            source,
            JournalPhase::FilesystemFinalized,
            true,
        )?;
        Ok(())
    }

    fn import_document_through_phase(
        &mut self,
        project_id: &str,
        source: ImportSource,
        stop_after: JournalPhase,
        finalize_before_phase: bool,
    ) -> Result<Option<DocumentSnapshot>, LibraryError> {
        let binding = self.required_writable_binding()?;
        let project = self.project_by_id(project_id)?;
        self.verified_project_path(&binding, project_id, &project.relative_path)?;
        let source = match source {
            ImportSource::ExternalPath { absolute_path } => {
                inspect_markdown_source(&absolute_path)?
            }
            ImportSource::TrackedFile {
                tracked_folder_id,
                relative_path,
            } => self.resolve_tracked_source(&tracked_folder_id, &relative_path)?,
        };
        let original_name = source
            .resolved_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| LibraryError::invalid_path("Source filename is invalid"))?;
        validate_document_name(original_name)?;
        let imported_at = now_millis();

        for sequence in 1..=10_000 {
            let name = collision_name(original_name, sequence)?;
            let relative = document_relative_path(&project.relative_path, &name);
            if self.document_destination_exists(&binding, &relative)? {
                continue;
            }
            let operation_id = Uuid::new_v4().to_string();
            let document_id = Uuid::new_v4().to_string();
            let temporary_relative = document_relative_path(
                &project.relative_path,
                &format!(".cairn-import-{operation_id}.tmp"),
            );
            let payload = OperationPayload::ImportDocument {
                document_id: document_id.clone(),
                project_id: project_id.to_owned(),
                relative_path: relative.clone(),
                source_path: source.provenance_path.clone(),
                source_resolved_path: source.resolved_path.clone(),
                source_identity: source.identity.clone(),
                source_fingerprint: source.fingerprint.clone(),
                imported_at,
            };
            self.insert_operation(
                &operation_id,
                &binding.library_id,
                "import_document",
                &payload,
                Some(&temporary_relative),
                Some(&source.fingerprint),
            )?;
            if stop_after == JournalPhase::IntentRecorded {
                return Ok(None);
            }
            let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
            let copied = match copy_stable_source(&source, &temporary) {
                Ok(copied) => copied,
                Err(error) => {
                    self.remove_operation(&operation_id)?;
                    return Err(error);
                }
            };
            self.update_operation(
                &operation_id,
                JournalPhase::TemporaryDurable,
                None,
                Some(&copied.identity),
            )?;
            if stop_after == JournalPhase::TemporaryDurable {
                return Ok(None);
            }
            self.verify_active_root(&binding)?;
            self.verified_project_path(&binding, project_id, &project.relative_path)?;
            let target = resolve_new(&binding.root_path, &relative)?;
            match rename_no_replace(&temporary, &target) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    remove_owned_file(&temporary, &copied.identity);
                    self.finish_operation(&operation_id)?;
                    continue;
                }
                Err(error) => return Err(LibraryError::io(error)),
            }
            sync_parent(&target)?;
            if finalize_before_phase {
                return Ok(None);
            }
            let target_identity = file_identity(&target)?;
            self.update_operation(
                &operation_id,
                JournalPhase::FilesystemFinalized,
                Some(&copied.fingerprint),
                target_identity.as_deref(),
            )?;
            if stop_after == JournalPhase::FilesystemFinalized {
                return Ok(None);
            }
            self.insert_document_metadata(DocumentMetadataCommit {
                operation_id: &operation_id,
                document_id: &document_id,
                library_id: &binding.library_id,
                project_id,
                relative_path: &relative,
                target: &target,
                disk_fingerprint: &copied.fingerprint,
                source_path: Some(&source.provenance_path),
                imported_at: Some(imported_at),
            })?;
            let document = self.document_by_id(&document_id)?;
            if stop_after == JournalPhase::MetadataCommitted {
                return Ok(Some(document));
            }
            self.finish_operation(&operation_id)?;
            return Ok(Some(document));
        }

        Err(LibraryError::new(
            "destination_busy",
            "Could not allocate an import filename",
        ))
    }

    pub fn add_tracked_folder(
        &mut self,
        path: &Path,
    ) -> Result<TrackedFolderSnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        if !path.is_absolute() {
            return Err(LibraryError::new(
                "tracked_folder_unavailable",
                "Tracked folders must use an absolute path",
            ));
        }
        let canonical = fs::canonicalize(path)
            .map_err(|error| LibraryError::new("tracked_folder_unavailable", error.to_string()))?;
        if !canonical.is_dir() {
            return Err(LibraryError::new(
                "tracked_folder_unavailable",
                "Tracked folder is not a directory",
            ));
        }
        if is_same_or_descendant(&binding.root_path, &canonical)
            || is_same_or_descendant(&canonical, &binding.root_path)
        {
            return Err(LibraryError::new(
                "tracked_folder_overlap",
                "Tracked folders cannot overlap the active library",
            ));
        }
        let folder_identity = file_identity(&canonical)?.ok_or_else(|| {
            LibraryError::new(
                "tracked_folder_unavailable",
                "Tracked folder identity unavailable",
            )
        })?;
        let display_name = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| path_text(&canonical));
        let id = Uuid::new_v4().to_string();
        let now = now_millis();
        self.database_mut()?.connection_mut().execute(
            "INSERT INTO tracked_folders (id, absolute_path, display_name, last_scan_at, path_key, folder_identity, created_at, updated_at) VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?6)",
            params![&id, path_text(&canonical), &display_name, absolute_path_key(&canonical), &folder_identity, now],
        ).map_err(database_conflict)?;
        Ok(TrackedFolderSnapshot {
            id,
            absolute_path: canonical,
            display_name,
            available: true,
            last_scan_at: None,
        })
    }

    pub fn list_tracked_folders(&self) -> Result<Vec<TrackedFolderSnapshot>, LibraryError> {
        let database = self.require_metadata()?;
        let mut statement = database.connection().prepare(
            "SELECT id, absolute_path, display_name, folder_identity, last_scan_at FROM tracked_folders ORDER BY lower(display_name), path_key",
        ).map_err(LibraryError::database)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(LibraryError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(LibraryError::database)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, absolute_path, display_name, expected_identity, last_scan_at)| {
                    let absolute_path = PathBuf::from(absolute_path);
                    let available = expected_identity.is_some()
                        && file_identity(&absolute_path).ok().flatten() == expected_identity;
                    TrackedFolderSnapshot {
                        id,
                        absolute_path,
                        display_name,
                        available,
                        last_scan_at,
                    }
                },
            )
            .collect())
    }

    pub fn remove_tracked_folder(&mut self, id: &str) -> Result<(), LibraryError> {
        let changed = self
            .database_mut()?
            .connection_mut()
            .execute("DELETE FROM tracked_folders WHERE id = ?1", [id])
            .map_err(LibraryError::database)?;
        if changed != 1 {
            return Err(LibraryError::new(
                "tracked_folder_not_found",
                "Tracked folder was not found",
            ));
        }
        Ok(())
    }

    pub fn list_tracked_folder_entries(
        &mut self,
        folder_id: &str,
        relative_directory: Option<&str>,
    ) -> Result<Vec<TrackedFolderEntry>, LibraryError> {
        let folder = self.tracked_folder_by_id(folder_id)?;
        self.verify_tracked_folder(&folder)?;
        let relative_directory = relative_directory.unwrap_or_default();
        let directory = if relative_directory.is_empty() {
            folder.absolute_path.clone()
        } else {
            let relative = validate_tracked_relative_path(relative_directory)?;
            fs::canonicalize(folder.absolute_path.join(relative)).map_err(|error| {
                LibraryError::new("tracked_folder_unavailable", error.to_string())
            })?
        };
        if !directory.is_dir() || !is_same_or_descendant(&folder.absolute_path, &directory) {
            return Err(LibraryError::new(
                "tracked_source_escape",
                "Tracked directory is outside its configured folder",
            ));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&directory)
            .map_err(|error| LibraryError::new("tracked_folder_unavailable", error.to_string()))?
        {
            let entry = entry.map_err(|error| {
                LibraryError::new("tracked_folder_unavailable", error.to_string())
            })?;
            let display_name = entry.file_name().to_string_lossy().into_owned();
            if display_name.starts_with('.') {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                LibraryError::new("tracked_folder_unavailable", error.to_string())
            })?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            let canonical = fs::canonicalize(entry.path()).map_err(|error| {
                LibraryError::new("tracked_folder_unavailable", error.to_string())
            })?;
            if !is_same_or_descendant(&folder.absolute_path, &canonical) {
                continue;
            }
            let is_directory = canonical.is_dir();
            let is_markdown = canonical.is_file()
                && canonical
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
            if !is_directory && !is_markdown {
                continue;
            }
            let relative_path = canonical
                .strip_prefix(&folder.absolute_path)
                .map_err(|_| {
                    LibraryError::new(
                        "tracked_source_escape",
                        "Tracked entry is outside its configured folder",
                    )
                })?
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            entries.push(TrackedFolderEntry {
                relative_path,
                display_name,
                is_directory,
            });
        }
        entries.sort_by(|left, right| {
            right.is_directory.cmp(&left.is_directory).then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
        });
        self.database_mut()?
            .connection_mut()
            .execute(
                "UPDATE tracked_folders SET last_scan_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now_millis(), folder_id],
            )
            .map_err(LibraryError::database)?;
        Ok(entries)
    }

    pub fn create_document_interrupted_for_test(
        &mut self,
        project_id: &str,
        name: &str,
        stop_after: JournalPhase,
    ) -> Result<(), LibraryError> {
        self.create_document_through_phase(project_id, name, stop_after, false)?;
        Ok(())
    }

    pub fn create_document_interrupted_after_finalize_before_phase_for_test(
        &mut self,
        project_id: &str,
        name: &str,
    ) -> Result<(), LibraryError> {
        self.create_document_through_phase(
            project_id,
            name,
            JournalPhase::FilesystemFinalized,
            true,
        )?;
        Ok(())
    }

    pub fn rename_document(
        &mut self,
        document_id: &str,
        name: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        let name = validate_document_name(name)?;
        let document = self.document_by_id(document_id)?;
        let project = self.project_for_document(document_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        validate_relative_path(&project.relative_path, 1)?;
        let relative = document_relative_path(&project.relative_path, &name);
        self.ensure_document_path_available(&binding.library_id, &relative, Some(document_id))?;
        let source = self.verified_document_path(&binding, document_id, &document.relative_path)?;
        let target = resolve_new(&binding.root_path, &relative)?;
        let operation_id = Uuid::new_v4().to_string();
        let expected = fingerprint(&source)?;
        let payload = OperationPayload::MoveDocument {
            document_id: document_id.to_owned(),
            target_project_id: project.id.clone(),
            from_relative_path: document.relative_path.clone(),
            to_relative_path: relative.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "rename_document",
            &payload,
            None,
            Some(&expected),
        )?;
        self.verify_active_root(&binding)?;
        self.verify_document_file(&source, document_id)?;
        case_aware_rename(&source, &target, &operation_id).map_err(map_conflict_io)?;
        let target_identity = file_identity(&target)?;
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&expected),
            target_identity.as_deref(),
        )?;
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "UPDATE documents SET relative_path = ?1, path_key = ?2, file_identity = ?3, disk_fingerprint = ?4, updated_at = ?5 WHERE id = ?6",
            params![relative, path_key(&relative), &target_identity, fingerprint(&target)?, now_millis(), document_id],
        ).map_err(database_conflict)?;
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now_millis(),
                    &operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.finish_operation(&operation_id)?;
        self.document_by_id(document_id)
    }

    pub fn move_document(
        &mut self,
        document_id: &str,
        target_project_id: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        let document = self.document_by_id(document_id)?;
        let target_project = self.project_by_id(target_project_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        validate_relative_path(&target_project.relative_path, 1)?;
        let name = Path::new(&document.relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| LibraryError::invalid_path("Document path is invalid"))?;
        let relative = document_relative_path(&target_project.relative_path, name);
        self.ensure_document_path_available(&binding.library_id, &relative, Some(document_id))?;
        let source = self.verified_document_path(&binding, document_id, &document.relative_path)?;
        let target = resolve_new(&binding.root_path, &relative)?;
        let operation_id = Uuid::new_v4().to_string();
        let expected = fingerprint(&source)?;
        let payload = OperationPayload::MoveDocument {
            document_id: document_id.to_owned(),
            target_project_id: target_project_id.to_owned(),
            from_relative_path: document.relative_path.clone(),
            to_relative_path: relative.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "move_document",
            &payload,
            None,
            Some(&expected),
        )?;
        self.verify_active_root(&binding)?;
        self.verify_document_file(&source, document_id)?;
        rename_no_replace(&source, &target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        let target_identity = file_identity(&target)?;
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&expected),
            target_identity.as_deref(),
        )?;
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction.execute(
            "UPDATE documents SET project_id = ?1, relative_path = ?2, path_key = ?3, file_identity = ?4, updated_at = ?5 WHERE id = ?6",
            params![target_project_id, relative, path_key(&relative), &target_identity, now_millis(), document_id],
        ).map_err(database_conflict)?;
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now_millis(),
                    &operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.finish_operation(&operation_id)?;
        self.document_by_id(document_id)
    }

    pub fn delete_document(&mut self, document_id: &str) -> Result<DeletedDocument, LibraryError> {
        let binding = self.required_writable_binding()?;
        let document = self.document_by_id(document_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        let source = self.verified_document_path(&binding, document_id, &document.relative_path)?;
        let operation_id = Uuid::new_v4().to_string();
        let expected = fingerprint(&source)?;
        let project_path = document
            .relative_path
            .split('/')
            .next()
            .ok_or_else(|| LibraryError::invalid_path("Document path is invalid"))?;
        let recovery_relative_path =
            document_relative_path(project_path, &format!(".cairn-recovery-{operation_id}.md"));
        let recovery_path = resolve_new(&binding.root_path, &recovery_relative_path)?;
        let payload = OperationPayload::DeleteDocument {
            document_id: document_id.to_owned(),
            relative_path: document.relative_path.clone(),
            recovery_relative_path: Some(recovery_relative_path.clone()),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "delete_document",
            &payload,
            Some(&recovery_relative_path),
            Some(&expected),
        )?;
        ensure_recovery_copy(&source, &recovery_path, &expected)?;
        let recovery_identity = file_identity(&recovery_path)?;
        self.update_operation(
            &operation_id,
            JournalPhase::TemporaryDurable,
            None,
            recovery_identity.as_deref(),
        )?;
        self.verify_active_root(&binding)?;
        self.verify_document_file(&source, document_id)?;
        recycle_file(&source)?;
        self.update_operation(&operation_id, JournalPhase::FilesystemFinalized, None, None)?;
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        transaction
            .execute("DELETE FROM documents WHERE id = ?1", [document_id])
            .map_err(LibraryError::database)?;
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now_millis(),
                    &operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        self.finish_operation(&operation_id)?;
        Ok(DeletedDocument {
            recovery_path: Some(recovery_path),
            recycled: true,
        })
    }

    pub fn reconcile(&mut self) -> Result<LibrarySnapshot, LibraryError> {
        let binding = self.required_writable_binding()?;
        let scanned = match scan(&binding.root_path) {
            Ok(scanned) => scanned,
            Err(error) => {
                if error.code() == "scan_transient" {
                    self.watcher_hints.request_scan();
                    return Err(error);
                }
                self.mode = LibraryMode::ReadOnly;
                self.read_only_reason = Some("root_unavailable".to_owned());
                return Err(error);
            }
        };
        self.reconcile_scanned(&binding, &scanned)?;
        self.snapshot()
    }

    pub fn reconcile_if_requested(&mut self) -> Result<Option<LibrarySnapshot>, LibraryError> {
        if self.watcher_hints.take_scan_request() {
            return self.reconcile().map(Some);
        }
        Ok(None)
    }

    pub fn pending_operation_count(&self) -> Result<i64, LibraryError> {
        let database = self.require_metadata()?;
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM pending_file_operations", [], |row| {
                row.get(0)
            })
            .map_err(LibraryError::database)
    }

    fn create_document_through_phase(
        &mut self,
        project_id: &str,
        name: &str,
        stop_after: JournalPhase,
        stop_before_finalize_record: bool,
    ) -> Result<Option<DocumentSnapshot>, LibraryError> {
        let binding = self.required_writable_binding()?;
        let name = validate_document_name(name)?;
        let project = self.project_by_id(project_id)?;
        self.verified_project_path(&binding, project_id, &project.relative_path)?;
        let relative = document_relative_path(&project.relative_path, &name);
        self.ensure_document_path_available(&binding.library_id, &relative, None)?;
        let operation_id = Uuid::new_v4().to_string();
        let document_id = Uuid::new_v4().to_string();
        let temporary_relative = document_relative_path(
            &project.relative_path,
            &format!(".cairn-{operation_id}.tmp"),
        );
        let payload = OperationPayload::CreateDocument {
            document_id: document_id.clone(),
            project_id: project_id.to_owned(),
            relative_path: relative.clone(),
        };
        self.insert_operation(
            &operation_id,
            &binding.library_id,
            "create_document",
            &payload,
            Some(&temporary_relative),
            Some("sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        )?;
        if stop_after == JournalPhase::IntentRecorded {
            return Ok(None);
        }
        self.verify_active_root(&binding)?;
        self.verified_project_path(&binding, project_id, &project.relative_path)?;
        let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
        write_durable(&temporary, b"")?;
        let temporary_identity = file_identity(&temporary)?;
        self.update_operation(
            &operation_id,
            JournalPhase::TemporaryDurable,
            None,
            temporary_identity.as_deref(),
        )?;
        if stop_after == JournalPhase::TemporaryDurable {
            return Ok(None);
        }
        let target = resolve_new(&binding.root_path, &relative)?;
        rename_no_replace(&temporary, &target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        let final_fingerprint = fingerprint(&target)?;
        if stop_before_finalize_record {
            return Ok(None);
        }
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&final_fingerprint),
            file_identity(&target)?.as_deref(),
        )?;
        if stop_after == JournalPhase::FilesystemFinalized {
            return Ok(None);
        }
        self.insert_document_metadata(DocumentMetadataCommit {
            operation_id: &operation_id,
            document_id: &document_id,
            library_id: &binding.library_id,
            project_id,
            relative_path: &relative,
            target: &target,
            disk_fingerprint: &final_fingerprint,
            source_path: None,
            imported_at: None,
        })?;
        if stop_after == JournalPhase::MetadataCommitted {
            return Ok(Some(self.document_by_id(&document_id)?));
        }
        self.finish_operation(&operation_id)?;
        Ok(Some(self.document_by_id(&document_id)?))
    }

    fn replay_pending_operations(&mut self) -> Result<(), LibraryError> {
        let Some(binding) = self.binding()? else {
            return Ok(());
        };
        if !binding.root_path.is_dir() {
            return Ok(());
        }
        let operations = {
            let database = self.require_metadata()?;
            let mut statement = database.connection().prepare(
                "SELECT id, phase, payload_json, temporary_path, expected_fingerprint, temporary_identity, finalized_identity FROM pending_file_operations ORDER BY created_at, id",
            ).map_err(LibraryError::database)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                })
                .map_err(LibraryError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(LibraryError::database)?;
            rows
        };
        for (
            id,
            phase_text,
            payload_json,
            temporary_path,
            expected_fingerprint,
            temporary_identity,
            finalized_identity,
        ) in operations
        {
            let phase = JournalPhase::parse(&phase_text)?;
            if phase == JournalPhase::CleanupComplete {
                self.remove_operation(&id)?;
                continue;
            }
            if phase == JournalPhase::MetadataCommitted {
                if let Ok(OperationPayload::SaveDocument {
                    intended_fingerprint,
                    backup_relative_path,
                    ..
                }) = serde_json::from_str::<OperationPayload>(&payload_json)
                {
                    let temporary_relative = temporary_path.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Save operation has no temporary path")
                    })?;
                    validate_relative_path(temporary_relative, 2)?;
                    validate_relative_path(&backup_relative_path, 2)?;
                    let temporary = resolve_new(&binding.root_path, temporary_relative)?;
                    let backup = resolve_new(&binding.root_path, &backup_relative_path)?;
                    cleanup_owned_artifact(&temporary, &intended_fingerprint)?;
                    if let Some(expected) = expected_fingerprint.as_deref() {
                        cleanup_owned_artifact(&backup, expected)?;
                    }
                }
                self.finish_operation(&id)?;
                continue;
            }
            let payload: OperationPayload = serde_json::from_str(&payload_json)
                .map_err(|error| LibraryError::new("journal_invalid", error.to_string()))?;
            match payload {
                OperationPayload::CreateProject {
                    project_id,
                    relative_path,
                } => {
                    validate_relative_path(&relative_path, 1)?;
                    let target = resolve_new(&binding.root_path, &relative_path)?;
                    if phase == JournalPhase::FilesystemFinalized {
                        if !target.is_dir() {
                            return Err(journal_mismatch("Finalized project target is missing"));
                        }
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized project target identity does not match the journal",
                        )?;
                    } else if target.exists() {
                        return Err(journal_mismatch(
                            "Unfinalized project target is already occupied",
                        ));
                    } else {
                        fs::create_dir(&target).map_err(map_conflict_io)?;
                        sync_parent(&target)?;
                        let identity = file_identity(&target)?;
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            None,
                            identity.as_deref(),
                        )?;
                    }
                    let identity = file_identity(&target)?;
                    self.commit_project_metadata(
                        &id,
                        &project_id,
                        &binding.library_id,
                        &relative_path,
                        identity.as_deref(),
                    )?;
                }
                OperationPayload::RenameProject {
                    project_id,
                    from_relative_path,
                    to_relative_path,
                } => {
                    validate_relative_path(&from_relative_path, 1)?;
                    validate_relative_path(&to_relative_path, 1)?;
                    if phase == JournalPhase::FilesystemFinalized {
                        let target = resolve_existing(&binding.root_path, &to_relative_path)
                            .map_err(|_| journal_mismatch("Finalized project target is missing"))?;
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized project target identity does not match the journal",
                        )?;
                        self.verify_project_file(&target, &project_id)?;
                    } else {
                        replay_move(
                            &binding.root_path,
                            &from_relative_path,
                            &to_relative_path,
                            &id,
                            None,
                            self.project_identity(&project_id)?.as_deref(),
                        )?;
                        let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                        let identity = file_identity(&target)?;
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            None,
                            identity.as_deref(),
                        )?;
                    }
                    let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                    update_project_metadata(
                        self.database_mut()?.connection_mut(),
                        &id,
                        &project_id,
                        &to_relative_path,
                        file_identity(&target)?,
                    )?;
                }
                OperationPayload::CreateDocument {
                    document_id,
                    project_id,
                    relative_path,
                } => {
                    validate_relative_path(&relative_path, 2)?;
                    let temporary_relative = temporary_path.ok_or_else(|| {
                        LibraryError::new(
                            "journal_invalid",
                            "Create operation has no temporary path",
                        )
                    })?;
                    validate_relative_path(&temporary_relative, 2)?;
                    let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
                    let target = resolve_new(&binding.root_path, &relative_path)?;
                    let expected = expected_fingerprint.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Create operation has no fingerprint")
                    })?;
                    if phase == JournalPhase::FilesystemFinalized {
                        if !target.exists() {
                            return Err(journal_mismatch("Finalized document target is missing"));
                        }
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized document target identity does not match the journal",
                        )?;
                    } else {
                        match (temporary.exists(), target.exists()) {
                            (false, false) => {
                                write_durable(&temporary, b"")?;
                                let identity = file_identity(&temporary)?;
                                self.update_operation(
                                    &id,
                                    JournalPhase::TemporaryDurable,
                                    None,
                                    identity.as_deref(),
                                )?;
                            }
                            (true, false) => {
                                verify_fingerprint(&temporary, expected)?;
                                if let Some(expected_identity) = temporary_identity.as_deref() {
                                    verify_expected_identity(
                                        &temporary,
                                        Some(expected_identity),
                                        "Temporary document identity does not match the journal",
                                    )?;
                                } else {
                                    let identity = file_identity(&temporary)?;
                                    self.update_operation(
                                        &id,
                                        JournalPhase::TemporaryDurable,
                                        None,
                                        identity.as_deref(),
                                    )?;
                                }
                            }
                            (false, true) => {
                                verify_expected_identity(
                                    &target,
                                    temporary_identity.as_deref(),
                                    "Unrecorded finalized document was not the durable temporary",
                                )?;
                            }
                            (true, true) => {
                                let temporary_identity = file_identity(&temporary)?;
                                if temporary_identity.is_some()
                                    && temporary_identity == file_identity(&target)?
                                    && fingerprint(&temporary)? == fingerprint(&target)?
                                {
                                    fs::remove_file(&temporary).map_err(LibraryError::io)?;
                                } else {
                                    return Err(journal_mismatch(
                                        "Temporary and final documents do not match",
                                    ));
                                }
                            }
                        }
                        if !target.exists() {
                            rename_no_replace(&temporary, &target).map_err(map_conflict_io)?;
                            sync_parent(&target)?;
                        }
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            Some(expected),
                            file_identity(&target)?.as_deref(),
                        )?;
                    }
                    verify_fingerprint(&target, expected)?;
                    self.insert_document_metadata(DocumentMetadataCommit {
                        operation_id: &id,
                        document_id: &document_id,
                        library_id: &binding.library_id,
                        project_id: &project_id,
                        relative_path: &relative_path,
                        target: &target,
                        disk_fingerprint: expected,
                        source_path: None,
                        imported_at: None,
                    })?;
                }
                OperationPayload::ImportDocument {
                    document_id,
                    project_id,
                    relative_path,
                    source_path,
                    source_resolved_path,
                    source_identity,
                    source_fingerprint,
                    imported_at,
                } => {
                    validate_relative_path(&relative_path, 2)?;
                    let temporary_relative = temporary_path.ok_or_else(|| {
                        LibraryError::new(
                            "journal_invalid",
                            "Import operation has no temporary path",
                        )
                    })?;
                    validate_relative_path(&temporary_relative, 2)?;
                    if expected_fingerprint.as_deref() != Some(source_fingerprint.as_str()) {
                        return Err(LibraryError::new(
                            "journal_invalid",
                            "Import source fingerprint does not match the journal",
                        ));
                    }
                    let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
                    let target = resolve_new(&binding.root_path, &relative_path)?;
                    if phase == JournalPhase::FilesystemFinalized {
                        verify_fingerprint(&target, &source_fingerprint)?;
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized import target identity does not match the journal",
                        )?;
                    } else {
                        match (temporary.exists(), target.exists()) {
                            (false, false) => {
                                let source = SourceDescriptor {
                                    provenance_path: source_path.clone(),
                                    resolved_path: source_resolved_path,
                                    identity: source_identity,
                                    fingerprint: source_fingerprint.clone(),
                                };
                                let copied = match copy_stable_source(&source, &temporary) {
                                    Ok(copied) => copied,
                                    Err(error)
                                        if matches!(
                                            error.code(),
                                            "source_unavailable"
                                                | "source_unreadable"
                                                | "source_changed"
                                        ) =>
                                    {
                                        self.remove_operation(&id)?;
                                        continue;
                                    }
                                    Err(error) => return Err(error),
                                };
                                self.update_operation(
                                    &id,
                                    JournalPhase::TemporaryDurable,
                                    None,
                                    Some(&copied.identity),
                                )?;
                            }
                            (true, false) => {
                                verify_fingerprint(&temporary, &source_fingerprint)?;
                                if let Some(expected_identity) = temporary_identity.as_deref() {
                                    verify_expected_identity(
                                        &temporary,
                                        Some(expected_identity),
                                        "Import temporary identity does not match the journal",
                                    )?;
                                } else {
                                    self.update_operation(
                                        &id,
                                        JournalPhase::TemporaryDurable,
                                        None,
                                        file_identity(&temporary)?.as_deref(),
                                    )?;
                                }
                            }
                            (false, true) => {
                                verify_fingerprint(&target, &source_fingerprint)?;
                                verify_expected_identity(
                                    &target,
                                    temporary_identity.as_deref(),
                                    "Unrecorded import target was not the durable temporary",
                                )?;
                            }
                            (true, true) if same_filesystem_object(&temporary, &target)? => {
                                verify_fingerprint(&temporary, &source_fingerprint)?;
                                fs::remove_file(&temporary).map_err(LibraryError::io)?;
                            }
                            (true, true) => {
                                return Err(journal_mismatch(
                                    "Import temporary and target are different files",
                                ));
                            }
                        }
                        if !target.exists() {
                            rename_no_replace(&temporary, &target).map_err(map_conflict_io)?;
                            sync_parent(&target)?;
                        }
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            Some(&source_fingerprint),
                            file_identity(&target)?.as_deref(),
                        )?;
                    }
                    self.insert_document_metadata(DocumentMetadataCommit {
                        operation_id: &id,
                        document_id: &document_id,
                        library_id: &binding.library_id,
                        project_id: &project_id,
                        relative_path: &relative_path,
                        target: &target,
                        disk_fingerprint: &source_fingerprint,
                        source_path: Some(&source_path),
                        imported_at: Some(imported_at),
                    })?;
                }
                OperationPayload::SaveDocument {
                    document_id,
                    relative_path,
                    session_generation,
                    revision,
                    intended_fingerprint,
                    backup_relative_path,
                } => {
                    validate_relative_path(&relative_path, 2)?;
                    validate_relative_path(&backup_relative_path, 2)?;
                    let temporary_relative = temporary_path.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Save operation has no temporary path")
                    })?;
                    validate_relative_path(temporary_relative, 2)?;
                    let expected = expected_fingerprint.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Save operation has no base hash")
                    })?;
                    let temporary = resolve_new(&binding.root_path, temporary_relative)?;
                    let backup = resolve_new(&binding.root_path, &backup_relative_path)?;
                    let target = resolve_new(&binding.root_path, &relative_path)?;
                    let snapshot = self.load_recovery_snapshot(&document_id)?.ok_or_else(|| {
                        LibraryError::new(
                            "recovery_missing",
                            "Save operation has no durable recovery snapshot",
                        )
                    })?;
                    if snapshot.session_generation != session_generation
                        || snapshot.revision != revision
                        || snapshot.base_fingerprint != expected
                        || snapshot.intended_disk_hash != intended_fingerprint
                    {
                        return Err(journal_mismatch(
                            "Save operation does not match its recovery snapshot",
                        ));
                    }

                    if self.external_conflict_hash(&document_id)?.is_some() {
                        cleanup_owned_artifact(&temporary, &intended_fingerprint)?;
                        cleanup_owned_artifact(&backup, expected)?;
                        self.remove_operation(&id)?;
                        continue;
                    }

                    if phase == JournalPhase::IntentRecorded {
                        if temporary.exists() {
                            verify_fingerprint(&temporary, &intended_fingerprint)?;
                        } else {
                            write_durable(&temporary, &snapshot.bytes)?;
                        }
                        self.update_operation(
                            &id,
                            JournalPhase::TemporaryDurable,
                            None,
                            file_identity(&temporary)?.as_deref(),
                        )?;
                    }

                    let target_fingerprint = if target.exists() {
                        Some(fingerprint(&target)?)
                    } else {
                        None
                    };
                    if phase != JournalPhase::FilesystemFinalized {
                        if target_fingerprint.as_deref() == Some(intended_fingerprint.as_str()) {
                            verify_fingerprint(&backup, expected)?;
                            self.update_operation(
                                &id,
                                JournalPhase::FilesystemFinalized,
                                Some(&intended_fingerprint),
                                file_identity(&target)?.as_deref(),
                            )?;
                        } else if target_fingerprint.as_deref() == Some(expected)
                            && file_identity(&target)? == self.document_identity(&document_id)?
                        {
                            if !temporary.exists() {
                                return Err(journal_mismatch(
                                    "Durable save temporary disappeared before replacement",
                                ));
                            }
                            let expected_identity = self.document_identity(&document_id)?;
                            match replace_if_unchanged(
                                &target,
                                &temporary,
                                &backup,
                                expected,
                                expected_identity.as_deref(),
                                &intended_fingerprint,
                            ) {
                                Ok(replaced) => self.update_operation(
                                    &id,
                                    JournalPhase::FilesystemFinalized,
                                    Some(&replaced.fingerprint),
                                    replaced.file_identity.as_deref(),
                                )?,
                                Err(error) if error.code() == "external_change" => {
                                    self.record_external_conflict(
                                        &id,
                                        &document_id,
                                        &snapshot,
                                        &target,
                                    )?;
                                    cleanup_owned_artifact(&temporary, &intended_fingerprint)?;
                                    cleanup_owned_artifact(&backup, expected)?;
                                    self.remove_operation(&id)?;
                                    continue;
                                }
                                Err(error) => return Err(error),
                            }
                        } else {
                            self.record_external_conflict(&id, &document_id, &snapshot, &target)?;
                            cleanup_owned_artifact(&temporary, &intended_fingerprint)?;
                            cleanup_owned_artifact(&backup, expected)?;
                            self.remove_operation(&id)?;
                            continue;
                        }
                    }

                    verify_fingerprint(&target, &intended_fingerprint)?;
                    let target_identity = file_identity(&target)?;
                    if phase == JournalPhase::FilesystemFinalized {
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized save target identity does not match the journal",
                        )?;
                    }
                    self.commit_saved_document(
                        &id,
                        &document_id,
                        &session_generation,
                        revision,
                        &intended_fingerprint,
                        target_identity.as_deref(),
                    )?;
                    cleanup_owned_artifact(&temporary, &intended_fingerprint)?;
                    cleanup_owned_artifact(&backup, expected)?;
                }
                OperationPayload::MoveDocument {
                    document_id,
                    target_project_id,
                    from_relative_path,
                    to_relative_path,
                } => {
                    validate_relative_path(&from_relative_path, 2)?;
                    validate_relative_path(&to_relative_path, 2)?;
                    let expected = expected_fingerprint.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Move operation has no fingerprint")
                    })?;
                    if phase == JournalPhase::FilesystemFinalized {
                        let target = resolve_existing(&binding.root_path, &to_relative_path)
                            .map_err(|_| journal_mismatch("Finalized move target is missing"))?;
                        verify_fingerprint(&target, expected)?;
                        verify_expected_identity(
                            &target,
                            finalized_identity.as_deref(),
                            "Finalized move target identity does not match the journal",
                        )?;
                    } else {
                        let expected_identity = self.document_identity(&document_id)?;
                        replay_move(
                            &binding.root_path,
                            &from_relative_path,
                            &to_relative_path,
                            &id,
                            Some(expected),
                            expected_identity.as_deref(),
                        )?;
                        let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            Some(expected),
                            file_identity(&target)?.as_deref(),
                        )?;
                    }
                    let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                    let transaction = self
                        .database_mut()?
                        .connection_mut()
                        .transaction()
                        .map_err(LibraryError::database)?;
                    transaction.execute(
                            "UPDATE documents SET project_id = ?1, relative_path = ?2, path_key = ?3, file_identity = ?4, disk_fingerprint = ?5, updated_at = ?6 WHERE id = ?7",
                            params![target_project_id, to_relative_path, path_key(&to_relative_path), file_identity(&target)?, expected, now_millis(), document_id],
                        ).map_err(database_conflict)?;
                    transaction.execute(
                        "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                        params![JournalPhase::MetadataCommitted.as_str(), now_millis(), &id],
                    ).map_err(LibraryError::database)?;
                    transaction.commit().map_err(LibraryError::database)?;
                }
                OperationPayload::DeleteDocument {
                    document_id,
                    relative_path,
                    recovery_relative_path,
                } => {
                    validate_relative_path(&relative_path, 2)?;
                    let recovery_relative_path =
                        recovery_relative_path.or(temporary_path).ok_or_else(|| {
                            LibraryError::new(
                                "journal_invalid",
                                "Delete operation has no recovery path",
                            )
                        })?;
                    validate_relative_path(&recovery_relative_path, 2)?;
                    let source = resolve_new(&binding.root_path, &relative_path)?;
                    let recovery = resolve_new(&binding.root_path, &recovery_relative_path)?;
                    let expected = expected_fingerprint.as_deref().ok_or_else(|| {
                        LibraryError::new("journal_invalid", "Delete operation has no fingerprint")
                    })?;
                    if phase == JournalPhase::IntentRecorded {
                        if !source.exists() {
                            return Err(journal_mismatch(
                                "Delete source disappeared before recovery was durable",
                            ));
                        }
                        ensure_recovery_copy(&source, &recovery, expected)?;
                        self.update_operation(
                            &id,
                            JournalPhase::TemporaryDurable,
                            None,
                            file_identity(&recovery)?.as_deref(),
                        )?;
                    } else {
                        verify_fingerprint(&recovery, expected)?;
                        verify_expected_identity(
                            &recovery,
                            temporary_identity.as_deref(),
                            "Recovery copy identity does not match the journal",
                        )?;
                    }
                    if phase != JournalPhase::FilesystemFinalized {
                        if source.exists() {
                            verify_fingerprint(&source, expected)?;
                            let expected_identity = self.document_identity(&document_id)?;
                            verify_expected_identity(
                                &source,
                                expected_identity.as_deref(),
                                "Delete source identity does not match metadata",
                            )?;
                            recycle_file(&source)?;
                        }
                        self.update_operation(&id, JournalPhase::FilesystemFinalized, None, None)?;
                    }
                    let transaction = self
                        .database_mut()?
                        .connection_mut()
                        .transaction()
                        .map_err(LibraryError::database)?;
                    transaction
                        .execute("DELETE FROM documents WHERE id = ?1", [&document_id])
                        .map_err(LibraryError::database)?;
                    transaction.execute(
                        "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                        params![JournalPhase::MetadataCommitted.as_str(), now_millis(), &id],
                    ).map_err(LibraryError::database)?;
                    transaction.commit().map_err(LibraryError::database)?;
                }
            }
            self.update_operation(&id, JournalPhase::MetadataCommitted, None, None)?;
            self.finish_operation(&id)?;
        }
        Ok(())
    }

    fn reconcile_scanned(
        &mut self,
        binding: &LibraryBinding,
        scanned: &[ScannedProject],
    ) -> Result<(), LibraryError> {
        let now = now_millis();
        let manifest = inventory_manifest(scanned);
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        reconcile_transaction(
            &transaction,
            &binding.library_id,
            scanned,
            &manifest,
            now,
            false,
        )?;
        transaction.commit().map_err(LibraryError::database)
    }

    fn insert_document_metadata(
        &mut self,
        commit: DocumentMetadataCommit<'_>,
    ) -> Result<(), LibraryError> {
        let target_identity = file_identity(commit.target)?;
        let now = now_millis();
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let existing = transaction
            .query_row(
                "SELECT library_id, project_id, relative_path, path_key, source_path, imported_at, disk_fingerprint, file_identity FROM documents WHERE id = ?1",
                [commit.document_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                },
            )
            .optional()
            .map_err(LibraryError::database)?;
        let expected_path_key = path_key(commit.relative_path);
        if let Some(existing) = existing {
            if existing
                != (
                    commit.library_id.to_owned(),
                    commit.project_id.to_owned(),
                    commit.relative_path.to_owned(),
                    expected_path_key.clone(),
                    commit.source_path.map(str::to_owned),
                    commit.imported_at,
                    commit.disk_fingerprint.to_owned(),
                    target_identity.clone(),
                )
            {
                return Err(journal_mismatch(
                    "Existing document metadata does not match the journal",
                ));
            }
        } else {
            transaction.execute(
                "INSERT INTO documents (id, library_id, project_id, relative_path, path_key, source_path, imported_at, disk_fingerprint, disk_revision, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?10)",
                params![commit.document_id, commit.library_id, commit.project_id, commit.relative_path, expected_path_key, commit.source_path, commit.imported_at, commit.disk_fingerprint, target_identity, now],
            ).map_err(database_conflict)?;
        }
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    JournalPhase::MetadataCommitted.as_str(),
                    now,
                    commit.operation_id
                ],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        Ok(())
    }

    fn commit_project_metadata(
        &mut self,
        operation_id: &str,
        project_id: &str,
        library_id: &str,
        relative_path: &str,
        identity: Option<&str>,
    ) -> Result<(), LibraryError> {
        let now = now_millis();
        let expected_path_key = path_key(relative_path);
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let existing = transaction
            .query_row(
                "SELECT library_id, relative_path, path_key, file_identity FROM projects WHERE id = ?1",
                [project_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(LibraryError::database)?;
        if let Some(existing) = existing {
            if existing
                != (
                    library_id.to_owned(),
                    relative_path.to_owned(),
                    expected_path_key.clone(),
                    identity.map(str::to_owned),
                )
            {
                return Err(journal_mismatch(
                    "Existing project metadata does not match the journal",
                ));
            }
        } else {
            transaction.execute(
                "INSERT INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![project_id, library_id, relative_path, expected_path_key, identity, now],
            ).map_err(database_conflict)?;
        }
        transaction
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![JournalPhase::MetadataCommitted.as_str(), now, operation_id],
            )
            .map_err(LibraryError::database)?;
        transaction.commit().map_err(LibraryError::database)?;
        Ok(())
    }

    fn insert_operation(
        &mut self,
        id: &str,
        library_id: &str,
        kind: &str,
        payload: &OperationPayload,
        temporary_path: Option<&str>,
        expected_fingerprint: Option<&str>,
    ) -> Result<(), LibraryError> {
        let payload = serde_json::to_string(payload)
            .map_err(|error| LibraryError::new("journal_invalid", error.to_string()))?;
        self.database_mut()?.connection_mut().execute(
            "INSERT INTO pending_file_operations (id, library_id, kind, phase, payload_json, expected_fingerprint, finalized_fingerprint, temporary_path, temporary_identity, finalized_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL, NULL, ?8, ?8)",
            params![id, library_id, kind, JournalPhase::IntentRecorded.as_str(), payload, expected_fingerprint, temporary_path, now_millis()],
        ).map_err(LibraryError::database)?;
        Ok(())
    }

    fn update_operation(
        &mut self,
        id: &str,
        phase: JournalPhase,
        fingerprint: Option<&str>,
        identity: Option<&str>,
    ) -> Result<(), LibraryError> {
        self.database_mut()?.connection_mut().execute(
            "UPDATE pending_file_operations SET phase = ?1, finalized_fingerprint = CASE WHEN ?1 = 'Filesystem finalized' THEN COALESCE(?2, finalized_fingerprint) ELSE finalized_fingerprint END, temporary_identity = CASE WHEN ?1 = 'Temporary durable' THEN COALESCE(?3, temporary_identity) ELSE temporary_identity END, finalized_identity = CASE WHEN ?1 = 'Filesystem finalized' THEN COALESCE(?3, finalized_identity) ELSE finalized_identity END, updated_at = ?4 WHERE id = ?5",
            params![phase.as_str(), fingerprint, identity, now_millis(), id],
        ).map_err(LibraryError::database)?;
        Ok(())
    }

    fn finish_operation(&mut self, id: &str) -> Result<(), LibraryError> {
        self.database_mut()?
            .connection_mut()
            .execute(
                "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
                params![JournalPhase::CleanupComplete.as_str(), now_millis(), id],
            )
            .map_err(LibraryError::database)?;
        self.remove_operation(id)
    }

    fn remove_operation(&mut self, id: &str) -> Result<(), LibraryError> {
        self.database_mut()?
            .connection_mut()
            .execute("DELETE FROM pending_file_operations WHERE id = ?1", [id])
            .map_err(LibraryError::database)?;
        Ok(())
    }

    fn binding(&self) -> Result<Option<LibraryBinding>, LibraryError> {
        let Some(database) = &self.database else {
            return Ok(None);
        };
        query_binding(database)
    }

    fn required_binding(&self) -> Result<LibraryBinding, LibraryError> {
        self.binding()?
            .ok_or_else(|| LibraryError::new("library_not_bound", "No library is bound"))
    }

    fn required_writable_binding(&mut self) -> Result<LibraryBinding, LibraryError> {
        self.require_writable()?;
        let binding = self.required_binding()?;
        self.verify_active_root(&binding)?;
        Ok(binding)
    }

    fn verify_active_root(&mut self, binding: &LibraryBinding) -> Result<(), LibraryError> {
        let identity = file_identity(&binding.root_path).ok().flatten();
        if identity.as_deref() == Some(binding.root_identity.as_str()) {
            return Ok(());
        }
        self.enter_read_only("root_identity_mismatch");
        Err(LibraryError::new(
            "root_identity_mismatch",
            "The bound library folder was replaced or moved",
        ))
    }

    fn verified_project_path(
        &self,
        binding: &LibraryBinding,
        project_id: &str,
        relative_path: &str,
    ) -> Result<PathBuf, LibraryError> {
        let path = resolve_existing(&binding.root_path, relative_path)?;
        self.verify_project_file(&path, project_id)?;
        Ok(path)
    }

    fn verify_project_file(&self, path: &Path, project_id: &str) -> Result<(), LibraryError> {
        let expected = self.project_identity(project_id)?;
        if expected.is_some() && file_identity(path)? == expected {
            return Ok(());
        }
        Err(LibraryError::new(
            "external_change",
            "The project folder changed outside Cairn.md; reconcile before modifying it",
        ))
    }

    fn project_identity(&self, project_id: &str) -> Result<Option<String>, LibraryError> {
        self.require_metadata()?
            .connection()
            .query_row(
                "SELECT file_identity FROM projects WHERE id = ?1",
                [project_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(LibraryError::database)
    }

    fn verified_document_path(
        &self,
        binding: &LibraryBinding,
        document_id: &str,
        relative_path: &str,
    ) -> Result<PathBuf, LibraryError> {
        let path = resolve_existing(&binding.root_path, relative_path)?;
        self.verify_document_file(&path, document_id)?;
        Ok(path)
    }

    fn verify_document_file(&self, path: &Path, document_id: &str) -> Result<(), LibraryError> {
        let (expected_identity, expected_fingerprint) = self
            .require_metadata()?
            .connection()
            .query_row(
                "SELECT file_identity, disk_fingerprint FROM documents WHERE id = ?1",
                [document_id],
                |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(LibraryError::database)?;
        if expected_identity.is_some()
            && file_identity(path)? == expected_identity
            && fingerprint(path)? == expected_fingerprint
        {
            return Ok(());
        }
        Err(LibraryError::new(
            "external_change",
            "The document changed outside Cairn.md; reconcile before modifying it",
        ))
    }

    fn document_identity(&self, document_id: &str) -> Result<Option<String>, LibraryError> {
        self.require_metadata()?
            .connection()
            .query_row(
                "SELECT file_identity FROM documents WHERE id = ?1",
                [document_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(LibraryError::database)
    }

    fn external_conflict_hash(&self, document_id: &str) -> Result<Option<String>, LibraryError> {
        self.require_metadata()?
            .connection()
            .query_row(
                "SELECT external_hash FROM external_conflicts WHERE document_id = ?1",
                [document_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(LibraryError::database)
    }

    fn project_by_id(&self, id: &str) -> Result<ProjectSnapshot, LibraryError> {
        self.snapshot()?
            .projects
            .into_iter()
            .find(|project| project.id == id)
            .ok_or_else(|| LibraryError::new("project_not_found", "Project not found"))
    }

    fn document_by_id(&self, id: &str) -> Result<DocumentSnapshot, LibraryError> {
        self.snapshot()?
            .projects
            .into_iter()
            .flat_map(|project| project.documents)
            .find(|document| document.id == id)
            .ok_or_else(|| LibraryError::new("document_not_found", "Document not found"))
    }

    fn project_for_document(&self, document_id: &str) -> Result<ProjectSnapshot, LibraryError> {
        self.snapshot()?
            .projects
            .into_iter()
            .find(|project| {
                project
                    .documents
                    .iter()
                    .any(|document| document.id == document_id)
            })
            .ok_or_else(|| LibraryError::new("document_not_found", "Document not found"))
    }

    fn ensure_project_path_available(
        &self,
        library_id: &str,
        relative_path: &str,
        excluding_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        ensure_path_available(
            self.require_metadata()?.connection(),
            "projects",
            library_id,
            &path_key(relative_path),
            excluding_id,
        )
    }

    fn ensure_document_path_available(
        &self,
        library_id: &str,
        relative_path: &str,
        excluding_id: Option<&str>,
    ) -> Result<(), LibraryError> {
        ensure_path_available(
            self.require_metadata()?.connection(),
            "documents",
            library_id,
            &path_key(relative_path),
            excluding_id,
        )
    }

    fn document_destination_exists(
        &self,
        binding: &LibraryBinding,
        relative_path: &str,
    ) -> Result<bool, LibraryError> {
        let indexed = self
            .require_metadata()?
            .connection()
            .query_row(
                "SELECT 1 FROM documents WHERE library_id = ?1 AND path_key = ?2 LIMIT 1",
                params![&binding.library_id, path_key(relative_path)],
                |_| Ok(()),
            )
            .optional()
            .map_err(LibraryError::database)?
            .is_some();
        if indexed {
            return Ok(true);
        }
        let target = resolve_new(&binding.root_path, relative_path)?;
        let target_name = target
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| LibraryError::invalid_path("Import target filename is invalid"))?;
        let parent = target
            .parent()
            .ok_or_else(|| LibraryError::invalid_path("Import target has no project"))?;
        for entry in fs::read_dir(parent).map_err(LibraryError::io)? {
            let entry = entry.map_err(LibraryError::io)?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(target_name))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn tracked_folder_by_id(&self, id: &str) -> Result<TrackedFolderRecord, LibraryError> {
        self.require_metadata()?
            .connection()
            .query_row(
                "SELECT absolute_path, folder_identity FROM tracked_folders WHERE id = ?1",
                [id],
                |row| {
                    Ok(TrackedFolderRecord {
                        absolute_path: PathBuf::from(row.get::<_, String>(0)?),
                        folder_identity: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(LibraryError::database)?
            .ok_or_else(|| {
                LibraryError::new("tracked_folder_not_found", "Tracked folder was not found")
            })
    }

    fn verify_tracked_folder(&self, folder: &TrackedFolderRecord) -> Result<(), LibraryError> {
        let actual_identity = file_identity(&folder.absolute_path)
            .map_err(|error| LibraryError::new("tracked_folder_unavailable", error.to_string()))?;
        if actual_identity != folder.folder_identity {
            return Err(LibraryError::new(
                "tracked_folder_unavailable",
                "Tracked folder was moved, replaced, or removed",
            ));
        }
        if let Some(binding) = self.binding()? {
            if is_same_or_descendant(&binding.root_path, &folder.absolute_path)
                || is_same_or_descendant(&folder.absolute_path, &binding.root_path)
            {
                return Err(LibraryError::new(
                    "tracked_folder_overlap",
                    "Tracked folder overlaps the active library",
                ));
            }
        }
        Ok(())
    }

    fn resolve_tracked_source(
        &self,
        folder_id: &str,
        relative_path: &str,
    ) -> Result<SourceDescriptor, LibraryError> {
        let folder = self.tracked_folder_by_id(folder_id)?;
        self.verify_tracked_folder(&folder)?;
        let relative = validate_tracked_relative_path(relative_path)?;
        let selected_path = folder.absolute_path.join(relative);
        let canonical = fs::canonicalize(&selected_path)
            .map_err(|error| LibraryError::new("tracked_folder_unavailable", error.to_string()))?;
        if !is_same_or_descendant(&folder.absolute_path, &canonical) {
            return Err(LibraryError::new(
                "tracked_source_escape",
                "Tracked file is outside its configured folder",
            ));
        }
        inspect_markdown_source(&selected_path)
    }

    fn require_metadata(&self) -> Result<&Database, LibraryError> {
        self.database.as_ref().ok_or_else(|| {
            LibraryError::new(
                "metadata_damaged",
                "Metadata is unavailable; writes are disabled",
            )
        })
    }

    fn database_mut(&mut self) -> Result<&mut Database, LibraryError> {
        self.database.as_mut().ok_or_else(|| {
            LibraryError::new(
                "metadata_damaged",
                "Metadata is unavailable; writes are disabled",
            )
        })
    }

    fn require_writable(&self) -> Result<(), LibraryError> {
        self.require_metadata()?;
        if self.mode != LibraryMode::Writable {
            return Err(LibraryError::new(
                self.read_only_reason
                    .as_deref()
                    .unwrap_or("library_read_only"),
                "The active library is read-only",
            ));
        }
        Ok(())
    }

    fn restart_watcher(&mut self, root: &Path) -> Result<(), LibraryError> {
        self.watcher = Some(LibraryWatcher::start(root, self.watcher_hints.clone())?);
        Ok(())
    }

    fn enter_read_only(&mut self, reason: &str) {
        self.mode = LibraryMode::ReadOnly;
        self.read_only_reason = Some(reason.to_owned());
        self.watcher = None;
    }

    fn preserve_metadata_evidence(&self) {
        if let Some(database) = &self.database {
            database.preserve_evidence();
        }
    }
}

#[derive(Clone)]
struct ExistingProject {
    id: String,
    path_key: String,
    file_identity: Option<String>,
}

#[derive(Clone)]
struct ExistingDocument {
    id: String,
    path_key: String,
    file_identity: Option<String>,
    fingerprint: String,
}

struct DocumentMetadataCommit<'a> {
    operation_id: &'a str,
    document_id: &'a str,
    library_id: &'a str,
    project_id: &'a str,
    relative_path: &'a str,
    target: &'a Path,
    disk_fingerprint: &'a str,
    source_path: Option<&'a str>,
    imported_at: Option<i64>,
}

struct TrackedFolderRecord {
    absolute_path: PathBuf,
    folder_identity: Option<String>,
}

fn query_recovery_snapshot(
    database: &Database,
    document_id: &str,
) -> Result<Option<RecoverySnapshot>, LibraryError> {
    database
        .connection()
        .query_row(
            "SELECT document_id, session_generation, revision, content, content_hash, base_fingerprint, intended_disk_hash, operation_id, lifecycle_state, durable_at FROM recovery_snapshots WHERE document_id = ?1",
            [document_id],
            |row| {
                let lifecycle = row.get::<_, String>(8)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    lifecycle,
                    row.get::<_, i64>(9)?,
                ))
            },
        )
        .optional()
        .map_err(LibraryError::database)?
        .map(
            |(
                document_id,
                session_generation,
                revision,
                bytes,
                content_hash,
                base_fingerprint,
                intended_disk_hash,
                operation_id,
                lifecycle,
                durable_at,
            )| {
                Ok(RecoverySnapshot {
                    document_id,
                    session_generation,
                    revision,
                    bytes,
                    content_hash,
                    base_fingerprint,
                    intended_disk_hash,
                    operation_id,
                    lifecycle_state: RecoveryLifecycle::parse(&lifecycle)?,
                    durable_at,
                })
            },
        )
        .transpose()
}

fn cleanup_owned_artifact(path: &Path, expected_fingerprint: &str) -> Result<(), LibraryError> {
    if !path.exists() {
        return Ok(());
    }
    verify_fingerprint(path, expected_fingerprint)?;
    fs::remove_file(path).map_err(LibraryError::io)?;
    sync_parent(path)
}

fn query_binding(database: &Database) -> Result<Option<LibraryBinding>, LibraryError> {
    database.connection().query_row(
        "SELECT id, root_path, root_identity, binding_generation FROM libraries ORDER BY updated_at DESC LIMIT 1",
        [],
        |row| {
            Ok(LibraryBinding {
                library_id: row.get(0)?,
                root_path: PathBuf::from(row.get::<_, String>(1)?),
                root_identity: row.get(2)?,
                generation: row.get(3)?,
            })
        },
    ).optional().map_err(LibraryError::database)
}

fn load_existing_projects(
    transaction: &rusqlite::Transaction<'_>,
    library_id: &str,
) -> Result<Vec<ExistingProject>, LibraryError> {
    let mut statement = transaction
        .prepare("SELECT id, path_key, file_identity FROM projects WHERE library_id = ?1")
        .map_err(LibraryError::database)?;
    let rows = statement
        .query_map([library_id], |row| {
            Ok(ExistingProject {
                id: row.get(0)?,
                path_key: row.get(1)?,
                file_identity: row.get(2)?,
            })
        })
        .map_err(LibraryError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(LibraryError::database)?;
    Ok(rows)
}

fn load_existing_documents(
    transaction: &rusqlite::Transaction<'_>,
    library_id: &str,
) -> Result<Vec<ExistingDocument>, LibraryError> {
    let mut statement = transaction
        .prepare("SELECT id, path_key, file_identity, disk_fingerprint FROM documents WHERE library_id = ?1")
        .map_err(LibraryError::database)?;
    let rows = statement
        .query_map([library_id], |row| {
            Ok(ExistingDocument {
                id: row.get(0)?,
                path_key: row.get(1)?,
                file_identity: row.get(2)?,
                fingerprint: row.get(3)?,
            })
        })
        .map_err(LibraryError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(LibraryError::database)?;
    Ok(rows)
}

fn reconcile_transaction(
    transaction: &rusqlite::Transaction<'_>,
    library_id: &str,
    scanned: &[ScannedProject],
    manifest: &str,
    now: i64,
    allow_confirmed_path_rebind: bool,
) -> Result<(), LibraryError> {
    let projects = load_existing_projects(transaction, library_id)?;
    let documents = load_existing_documents(transaction, library_id)?;
    let mut used_projects = HashSet::new();
    let mut project_ids = vec![None; scanned.len()];

    for (index, candidate) in scanned.iter().enumerate() {
        let Some(identity) = candidate.file_identity.as_ref() else {
            continue;
        };
        let existing_matches = projects
            .iter()
            .filter(|row| row.file_identity.as_ref() == Some(identity))
            .collect::<Vec<_>>();
        let scanned_matches = scanned
            .iter()
            .filter(|row| row.file_identity.as_ref() == Some(identity))
            .count();
        if existing_matches.len() == 1 && scanned_matches == 1 {
            project_ids[index] = Some(existing_matches[0].id.clone());
            used_projects.insert(existing_matches[0].id.clone());
        }
    }
    for (index, candidate) in scanned.iter().enumerate() {
        if project_ids[index].is_some() {
            continue;
        }
        let key = path_key(&candidate.relative_path);
        if let Some(existing) = projects.iter().find(|row| {
            !used_projects.contains(&row.id)
                && row.path_key == key
                && (allow_confirmed_path_rebind
                    || identities_compatible(
                        row.file_identity.as_ref(),
                        candidate.file_identity.as_ref(),
                    ))
        }) {
            project_ids[index] = Some(existing.id.clone());
            used_projects.insert(existing.id.clone());
        }
    }

    let flattened = scanned
        .iter()
        .enumerate()
        .flat_map(|(project_index, project)| {
            project
                .documents
                .iter()
                .map(move |document| (project_index, document))
        })
        .collect::<Vec<_>>();
    let mut used_documents = HashSet::new();
    let mut document_ids = vec![None; flattened.len()];
    for (index, (_, candidate)) in flattened.iter().enumerate() {
        let Some(identity) = candidate.file_identity.as_ref() else {
            continue;
        };
        let existing_matches = documents
            .iter()
            .filter(|row| row.file_identity.as_ref() == Some(identity))
            .collect::<Vec<_>>();
        let scanned_matches = flattened
            .iter()
            .filter(|(_, row)| row.file_identity.as_ref() == Some(identity))
            .count();
        if existing_matches.len() == 1 && scanned_matches == 1 {
            document_ids[index] = Some(existing_matches[0].id.clone());
            used_documents.insert(existing_matches[0].id.clone());
        }
    }
    for (index, (_, candidate)) in flattened.iter().enumerate() {
        if document_ids[index].is_some() {
            continue;
        }
        let key = path_key(&candidate.relative_path);
        if let Some(existing) = documents.iter().find(|row| {
            let replacement_is_unambiguous =
                candidate
                    .file_identity
                    .as_ref()
                    .is_some_and(|candidate_identity| {
                        flattened
                            .iter()
                            .filter(|(_, scanned)| {
                                scanned.file_identity.as_ref() == Some(candidate_identity)
                            })
                            .count()
                            == 1
                            && !documents.iter().any(|other| {
                                other.id != row.id
                                    && other.file_identity.as_ref() == Some(candidate_identity)
                            })
                            && row.file_identity.as_ref().is_none_or(|previous_identity| {
                                !flattened.iter().any(|(_, scanned)| {
                                    scanned.file_identity.as_ref() == Some(previous_identity)
                                })
                            })
                    });
            !used_documents.contains(&row.id)
                && row.path_key == key
                && (allow_confirmed_path_rebind
                    || identities_compatible(
                        row.file_identity.as_ref(),
                        candidate.file_identity.as_ref(),
                    )
                    || replacement_is_unambiguous)
        }) {
            document_ids[index] = Some(existing.id.clone());
            used_documents.insert(existing.id.clone());
        }
    }
    for (index, (_, candidate)) in flattened.iter().enumerate() {
        if document_ids[index].is_some() {
            continue;
        }
        let existing_matches = documents
            .iter()
            .filter(|row| {
                !used_documents.contains(&row.id) && row.fingerprint == candidate.fingerprint
            })
            .collect::<Vec<_>>();
        let scanned_matches = flattened
            .iter()
            .enumerate()
            .filter(|(other_index, (_, row))| {
                document_ids[*other_index].is_none() && row.fingerprint == candidate.fingerprint
            })
            .count();
        if existing_matches.len() == 1 && scanned_matches == 1 {
            document_ids[index] = Some(existing_matches[0].id.clone());
            used_documents.insert(existing_matches[0].id.clone());
        }
    }

    transaction
        .execute(
            "UPDATE documents SET path_key = '@reconcile-document:' || id WHERE library_id = ?1",
            [library_id],
        )
        .map_err(LibraryError::database)?;
    transaction
        .execute(
            "UPDATE projects SET path_key = '@reconcile-project:' || id WHERE library_id = ?1",
            [library_id],
        )
        .map_err(LibraryError::database)?;

    for (index, candidate) in scanned.iter().enumerate() {
        let id = project_ids[index]
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        project_ids[index] = Some(id.clone());
        used_projects.insert(id.clone());
        transaction.execute(
            "INSERT INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) ON CONFLICT(id) DO UPDATE SET relative_path = excluded.relative_path, path_key = excluded.path_key, file_identity = excluded.file_identity, updated_at = excluded.updated_at",
            params![id, library_id, candidate.relative_path, path_key(&candidate.relative_path), candidate.file_identity, now],
        ).map_err(database_conflict)?;
    }
    for (index, (project_index, candidate)) in flattened.iter().enumerate() {
        let id = document_ids[index]
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        used_documents.insert(id.clone());
        let project_id = project_ids[*project_index]
            .as_ref()
            .expect("project assignment");
        transaction.execute(
            "INSERT INTO documents (id, library_id, project_id, relative_path, path_key, source_path, imported_at, disk_fingerprint, disk_revision, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, 0, ?7, ?8, ?8) ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, relative_path = excluded.relative_path, path_key = excluded.path_key, disk_fingerprint = excluded.disk_fingerprint, file_identity = excluded.file_identity, updated_at = excluded.updated_at",
            params![id, library_id, project_id, candidate.relative_path, path_key(&candidate.relative_path), candidate.fingerprint, candidate.file_identity, now],
        ).map_err(database_conflict)?;
    }
    for row in &documents {
        if !used_documents.contains(&row.id) {
            transaction
                .execute("DELETE FROM documents WHERE id = ?1", [&row.id])
                .map_err(LibraryError::database)?;
        }
    }
    for row in &projects {
        if !used_projects.contains(&row.id) {
            transaction
                .execute("DELETE FROM projects WHERE id = ?1", [&row.id])
                .map_err(LibraryError::database)?;
        }
    }
    transaction
        .execute(
            "UPDATE libraries SET manifest_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![manifest, now, library_id],
        )
        .map_err(LibraryError::database)?;
    Ok(())
}

fn identities_compatible(left: Option<&String>, right: Option<&String>) -> bool {
    left.is_none() || right.is_none() || left == right
}

fn replay_move(
    root: &Path,
    from_relative_path: &str,
    to_relative_path: &str,
    operation_id: &str,
    expected_fingerprint: Option<&str>,
    expected_identity: Option<&str>,
) -> Result<(), LibraryError> {
    let source = resolve_new(root, from_relative_path)?;
    let target = resolve_new(root, to_relative_path)?;
    if path_key(from_relative_path) == path_key(to_relative_path) {
        let temporary = source.with_file_name(format!(".cairn-case-{operation_id}.tmp"));
        if temporary.exists() {
            verify_expected_identity(
                &temporary,
                expected_identity,
                "Case-rename temporary identity does not match metadata",
            )?;
            if source.exists() {
                if !same_filesystem_object(&source, &temporary)? {
                    return Err(journal_mismatch(
                        "Case-rename source conflicts with its temporary file",
                    ));
                }
                fs::remove_file(&source).map_err(LibraryError::io)?;
            }
            if target.exists() {
                if !same_filesystem_object(&target, &temporary)? {
                    return Err(journal_mismatch(
                        "Case-rename target conflicts with its temporary file",
                    ));
                }
                fs::remove_file(&temporary).map_err(LibraryError::io)?;
            } else {
                rename_no_replace(&temporary, &target).map_err(map_conflict_io)?;
                sync_parent(&target)?;
            }
        } else if source.exists() {
            verify_expected_identity(
                &source,
                expected_identity,
                "Rename source identity does not match metadata",
            )?;
            if let Some(expected) = expected_fingerprint {
                verify_fingerprint(&source, expected)?;
            }
            case_aware_rename(&source, &target, operation_id).map_err(map_conflict_io)?;
        } else if !target.exists() {
            return Err(journal_mismatch("Case-only rename target is missing"));
        }
    } else {
        match (source.exists(), target.exists()) {
            (true, false) => {
                verify_expected_identity(
                    &source,
                    expected_identity,
                    "Move source identity does not match metadata",
                )?;
                if let Some(expected) = expected_fingerprint {
                    verify_fingerprint(&source, expected)?;
                }
                rename_no_replace(&source, &target).map_err(map_conflict_io)?;
                sync_parent(&target)?;
            }
            (false, true) => verify_expected_identity(
                &target,
                expected_identity,
                "Move target identity does not match metadata",
            )?,
            (true, true) if same_filesystem_object(&source, &target)? => {
                if let Some(expected) = expected_fingerprint {
                    verify_fingerprint(&source, expected)?;
                    verify_fingerprint(&target, expected)?;
                }
                fs::remove_file(&source).map_err(LibraryError::io)?;
            }
            (true, true) => return Err(journal_mismatch("Move source and target both exist")),
            (false, false) => return Err(journal_mismatch("Move source and target are missing")),
        }
    }
    if let Some(expected) = expected_fingerprint {
        verify_fingerprint(&target, expected)?;
    }
    verify_expected_identity(
        &target,
        expected_identity,
        "Move target identity does not match metadata",
    )?;
    Ok(())
}

fn same_filesystem_object(left: &Path, right: &Path) -> Result<bool, LibraryError> {
    let left_identity = file_identity(left)?;
    Ok(left_identity.is_some() && left_identity == file_identity(right)?)
}

fn ensure_recovery_copy(
    source: &Path,
    recovery: &Path,
    expected_fingerprint: &str,
) -> Result<(), LibraryError> {
    verify_fingerprint(source, expected_fingerprint)?;
    if recovery.exists() {
        return verify_fingerprint(recovery, expected_fingerprint);
    }
    copy_durable_no_replace(source, recovery)?;
    sync_parent(recovery)?;
    verify_fingerprint(recovery, expected_fingerprint)
}

fn verify_fingerprint(path: &Path, expected: &str) -> Result<(), LibraryError> {
    if fingerprint(path)? != expected {
        return Err(journal_mismatch(
            "Filesystem content no longer matches the journal",
        ));
    }
    Ok(())
}

fn verify_expected_identity(
    path: &Path,
    expected: Option<&str>,
    message: &str,
) -> Result<(), LibraryError> {
    let expected = expected
        .ok_or_else(|| LibraryError::new("journal_invalid", "Journal file identity is missing"))?;
    if file_identity(path)?.as_deref() != Some(expected) {
        return Err(journal_mismatch(message));
    }
    Ok(())
}

fn update_project_metadata(
    connection: &mut rusqlite::Connection,
    operation_id: &str,
    project_id: &str,
    relative_path: &str,
    identity: Option<String>,
) -> Result<(), LibraryError> {
    let transaction = connection.transaction().map_err(LibraryError::database)?;
    transaction.execute(
        "UPDATE projects SET relative_path = ?1, path_key = ?2, file_identity = ?3, updated_at = ?4 WHERE id = ?5",
        params![relative_path, path_key(relative_path), identity, now_millis(), project_id],
    ).map_err(database_conflict)?;
    let paths = {
        let mut statement = transaction
            .prepare("SELECT id, relative_path FROM documents WHERE project_id = ?1")
            .map_err(LibraryError::database)?;
        let rows = statement
            .query_map([project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(LibraryError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(LibraryError::database)?;
        rows
    };
    for (document_id, old_path) in paths {
        validate_relative_path(&old_path, 2)?;
        let file_name = old_path
            .split('/')
            .nth(1)
            .ok_or_else(|| LibraryError::invalid_path("Stored document path is invalid"))?;
        let updated = document_relative_path(relative_path, file_name);
        transaction.execute(
            "UPDATE documents SET relative_path = ?1, path_key = ?2, updated_at = ?3 WHERE id = ?4",
            params![updated, path_key(&updated), now_millis(), document_id],
        ).map_err(database_conflict)?;
    }
    transaction
        .execute(
            "UPDATE pending_file_operations SET phase = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                JournalPhase::MetadataCommitted.as_str(),
                now_millis(),
                operation_id
            ],
        )
        .map_err(LibraryError::database)?;
    transaction.commit().map_err(LibraryError::database)
}

fn journal_mismatch(message: &str) -> LibraryError {
    LibraryError::new("journal_mismatch", message)
}

fn ensure_path_available(
    connection: &rusqlite::Connection,
    table: &str,
    library_id: &str,
    key: &str,
    excluding_id: Option<&str>,
) -> Result<(), LibraryError> {
    let query = format!("SELECT id FROM {table} WHERE library_id = ?1 AND path_key = ?2 LIMIT 1");
    let existing: Option<String> = connection
        .query_row(&query, params![library_id, key], |row| row.get(0))
        .optional()
        .map_err(LibraryError::database)?;
    if existing
        .as_deref()
        .is_some_and(|id| Some(id) != excluding_id)
    {
        return Err(LibraryError::new(
            "path_conflict",
            "A matching path already exists",
        ));
    }
    Ok(())
}

fn inventory_manifest(scanned: &[ScannedProject]) -> String {
    let mut values = Vec::new();
    for project in scanned {
        values.push(format!("p:{}", path_key(&project.relative_path)));
        for document in &project.documents {
            values.push(format!(
                "d:{}:{}",
                path_key(&document.relative_path),
                document.fingerprint
            ));
        }
    }
    values.sort();
    format!("sha256:{:x}", Sha256::digest(values.join("\n")))
}

fn state_token(snapshot: &LibrarySnapshot) -> String {
    let mut values = Vec::new();
    if let Some(binding) = &snapshot.binding {
        values.push(format!("{}:{}", binding.library_id, binding.generation));
    }
    for project in &snapshot.projects {
        values.push(format!("p:{}:{}", project.id, project.relative_path));
        for document in &project.documents {
            values.push(format!(
                "d:{}:{}:{}",
                document.id, document.relative_path, document.disk_fingerprint
            ));
        }
    }
    format!("sha256:{:x}", Sha256::digest(values.join("\n")))
}

fn relink_state_token(snapshot: &LibrarySnapshot, candidate_manifest: &str) -> String {
    format!(
        "sha256:{:x}",
        Sha256::digest(format!("{}\n{candidate_manifest}", state_token(snapshot)))
    )
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn probe_error(probe: &CandidateRootProbe) -> LibraryError {
    let code = probe.reason.as_deref().unwrap_or("root_incompatible");
    LibraryError::new(
        code.split(':').next().unwrap_or(code),
        "Library root cannot be bound",
    )
}

fn map_conflict_io(error: std::io::Error) -> LibraryError {
    if error.kind() == std::io::ErrorKind::AlreadyExists {
        LibraryError::new("path_conflict", "A matching path already exists")
    } else {
        LibraryError::io(error)
    }
}

fn database_conflict(error: rusqlite::Error) -> LibraryError {
    if matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    ) {
        LibraryError::new("path_conflict", "A matching path already exists")
    } else {
        LibraryError::database(error)
    }
}
