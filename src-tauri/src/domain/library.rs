use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::project::{
    document_relative_path, path_key, validate_document_name, validate_project_name,
    validate_relative_path,
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
        self.update_operation(&operation_id, JournalPhase::FilesystemFinalized, None)?;
        let identity = file_identity(&target)?;
        self.database_mut()?.connection_mut().execute(
            "INSERT INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![&id, &binding.library_id, &name, path_key(&name), identity, now_millis()],
        ).map_err(database_conflict)?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
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
        self.update_operation(&operation_id, JournalPhase::FilesystemFinalized, None)?;
        let identity = file_identity(&target)?;
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
        transaction.commit().map_err(LibraryError::database)?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
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
        let source =
            self.verified_document_path(&binding, document_id, &document.relative_path)?;
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
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&expected),
        )?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE documents SET relative_path = ?1, path_key = ?2, file_identity = ?3, disk_fingerprint = ?4, updated_at = ?5 WHERE id = ?6",
            params![relative, path_key(&relative), file_identity(&target)?, fingerprint(&target)?, now_millis(), document_id],
        ).map_err(database_conflict)?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
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
        let source =
            self.verified_document_path(&binding, document_id, &document.relative_path)?;
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
        self.update_operation(
            &operation_id,
            JournalPhase::FilesystemFinalized,
            Some(&expected),
        )?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE documents SET project_id = ?1, relative_path = ?2, path_key = ?3, file_identity = ?4, updated_at = ?5 WHERE id = ?6",
            params![target_project_id, relative, path_key(&relative), file_identity(&target)?, now_millis(), document_id],
        ).map_err(database_conflict)?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
        self.finish_operation(&operation_id)?;
        self.document_by_id(document_id)
    }

    pub fn delete_document(&mut self, document_id: &str) -> Result<DeletedDocument, LibraryError> {
        let binding = self.required_writable_binding()?;
        let document = self.document_by_id(document_id)?;
        validate_relative_path(&document.relative_path, 2)?;
        let source =
            self.verified_document_path(&binding, document_id, &document.relative_path)?;
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
        self.update_operation(&operation_id, JournalPhase::TemporaryDurable, None)?;
        self.verify_active_root(&binding)?;
        self.verify_document_file(&source, document_id)?;
        recycle_file(&source)?;
        self.update_operation(&operation_id, JournalPhase::FilesystemFinalized, None)?;
        self.database_mut()?
            .connection_mut()
            .execute("DELETE FROM documents WHERE id = ?1", [document_id])
            .map_err(LibraryError::database)?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
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
        self.update_operation(&operation_id, JournalPhase::TemporaryDurable, None)?;
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
        )?;
        if stop_after == JournalPhase::FilesystemFinalized {
            return Ok(None);
        }
        self.insert_document_metadata(
            &document_id,
            &binding.library_id,
            project_id,
            &relative,
            &target,
            &final_fingerprint,
        )?;
        self.update_operation(&operation_id, JournalPhase::MetadataCommitted, None)?;
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
                "SELECT id, phase, payload_json, temporary_path, expected_fingerprint FROM pending_file_operations ORDER BY created_at, id",
            ).map_err(LibraryError::database)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                })
                .map_err(LibraryError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(LibraryError::database)?;
            rows
        };
        for (id, phase_text, payload_json, temporary_path, expected_fingerprint) in operations {
            let phase = JournalPhase::parse(&phase_text)?;
            if phase == JournalPhase::CleanupComplete {
                self.remove_operation(&id)?;
                continue;
            }
            if phase == JournalPhase::MetadataCommitted {
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
                    if phase == JournalPhase::FilesystemFinalized && !target.exists() {
                        return Err(journal_mismatch("Finalized project target is missing"));
                    }
                    if !target.exists() {
                        fs::create_dir(&target).map_err(map_conflict_io)?;
                        sync_parent(&target)?;
                    }
                    if !target.is_dir() {
                        return Err(journal_mismatch("Project target is not a directory"));
                    }
                    if phase != JournalPhase::FilesystemFinalized {
                        self.update_operation(&id, JournalPhase::FilesystemFinalized, None)?;
                    }
                    self.database_mut()?.connection_mut().execute(
                        "INSERT OR IGNORE INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                        params![project_id, binding.library_id, relative_path, path_key(&relative_path), file_identity(&target)?, now_millis()],
                    ).map_err(database_conflict)?;
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
                        if !target.is_dir() {
                            return Err(journal_mismatch("Project target is not a directory"));
                        }
                    } else {
                        replay_move(
                            &binding.root_path,
                            &from_relative_path,
                            &to_relative_path,
                            &id,
                            None,
                        )?;
                        self.update_operation(&id, JournalPhase::FilesystemFinalized, None)?;
                    }
                    let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                    update_project_metadata(
                        self.database_mut()?.connection_mut(),
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
                    } else {
                        match (temporary.exists(), target.exists()) {
                            (false, false) => write_durable(&temporary, b"")?,
                            (true, false) => verify_fingerprint(&temporary, expected)?,
                            (false, true) => {}
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
                        )?;
                    }
                    verify_fingerprint(&target, expected)?;
                    self.insert_document_metadata(
                        &document_id,
                        &binding.library_id,
                        &project_id,
                        &relative_path,
                        &target,
                        expected,
                    )?;
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
                    } else {
                        replay_move(
                            &binding.root_path,
                            &from_relative_path,
                            &to_relative_path,
                            &id,
                            Some(expected),
                        )?;
                        self.update_operation(
                            &id,
                            JournalPhase::FilesystemFinalized,
                            Some(expected),
                        )?;
                    }
                    let target = resolve_existing(&binding.root_path, &to_relative_path)?;
                    self.database_mut()?.connection_mut().execute(
                        "UPDATE documents SET project_id = ?1, relative_path = ?2, path_key = ?3, file_identity = ?4, disk_fingerprint = ?5, updated_at = ?6 WHERE id = ?7",
                        params![target_project_id, to_relative_path, path_key(&to_relative_path), file_identity(&target)?, expected, now_millis(), document_id],
                    ).map_err(database_conflict)?;
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
                        self.update_operation(&id, JournalPhase::TemporaryDurable, None)?;
                    } else {
                        verify_fingerprint(&recovery, expected)?;
                    }
                    if phase != JournalPhase::FilesystemFinalized {
                        if source.exists() {
                            verify_fingerprint(&source, expected)?;
                            recycle_file(&source)?;
                        }
                        self.update_operation(&id, JournalPhase::FilesystemFinalized, None)?;
                    }
                    self.database_mut()?
                        .connection_mut()
                        .execute("DELETE FROM documents WHERE id = ?1", [&document_id])
                        .map_err(LibraryError::database)?;
                }
            }
            self.update_operation(&id, JournalPhase::MetadataCommitted, None)?;
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
        id: &str,
        library_id: &str,
        project_id: &str,
        relative_path: &str,
        target: &Path,
        disk_fingerprint: &str,
    ) -> Result<(), LibraryError> {
        self.database_mut()?.connection_mut().execute(
            "INSERT OR IGNORE INTO documents (id, library_id, project_id, relative_path, path_key, source_path, imported_at, disk_fingerprint, disk_revision, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, 0, ?7, ?8, ?8)",
            params![id, library_id, project_id, relative_path, path_key(relative_path), disk_fingerprint, file_identity(target)?, now_millis()],
        ).map_err(database_conflict)?;
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
            "INSERT INTO pending_file_operations (id, library_id, kind, phase, payload_json, expected_fingerprint, finalized_fingerprint, temporary_path, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?8)",
            params![id, library_id, kind, JournalPhase::IntentRecorded.as_str(), payload, expected_fingerprint, temporary_path, now_millis()],
        ).map_err(LibraryError::database)?;
        Ok(())
    }

    fn update_operation(
        &mut self,
        id: &str,
        phase: JournalPhase,
        fingerprint: Option<&str>,
    ) -> Result<(), LibraryError> {
        self.database_mut()?.connection_mut().execute(
            "UPDATE pending_file_operations SET phase = ?1, finalized_fingerprint = CASE WHEN ?1 = 'Filesystem finalized' THEN COALESCE(?2, finalized_fingerprint) ELSE finalized_fingerprint END, updated_at = ?3 WHERE id = ?4",
            params![phase.as_str(), fingerprint, now_millis(), id],
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
        let expected = self
            .require_metadata()?
            .connection()
            .query_row(
                "SELECT file_identity FROM projects WHERE id = ?1",
                [project_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(LibraryError::database)?;
        if expected.is_some() && file_identity(path)? == expected {
            return Ok(());
        }
        Err(LibraryError::new(
            "external_change",
            "The project folder changed outside Cairn.md; reconcile before modifying it",
        ))
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
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                    ))
                },
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
            let replacement_is_unambiguous = candidate.file_identity.as_ref().is_some_and(
                |candidate_identity| {
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
                },
            );
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
) -> Result<(), LibraryError> {
    let source = resolve_new(root, from_relative_path)?;
    let target = resolve_new(root, to_relative_path)?;
    if path_key(from_relative_path) == path_key(to_relative_path) {
        let temporary = source.with_file_name(format!(".cairn-case-{operation_id}.tmp"));
        if temporary.exists() {
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
                if let Some(expected) = expected_fingerprint {
                    verify_fingerprint(&source, expected)?;
                }
                rename_no_replace(&source, &target).map_err(map_conflict_io)?;
                sync_parent(&target)?;
            }
            (false, true) => {}
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

fn update_project_metadata(
    connection: &mut rusqlite::Connection,
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
