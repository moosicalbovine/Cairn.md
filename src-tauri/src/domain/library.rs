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
};
use crate::infrastructure::database::{Database, DatabaseOpen};
use crate::infrastructure::filesystem::{
    case_aware_rename, file_identity, fingerprint, probe_candidate, rename_no_replace,
    resolve_existing, resolve_new, scan, sync_parent, write_durable, CandidateRootProbe,
    ScannedDocument, ScannedProject,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedDocument {
    pub recovery_path: PathBuf,
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
    CreateDocument {
        document_id: String,
        project_id: String,
        relative_path: String,
    },
}

pub struct LibraryService {
    app_data_dir: PathBuf,
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
            app_data_dir,
            database,
            mode,
            read_only_reason,
            watcher_hints: WatcherHints::default(),
            watcher: None,
        };

        if service.database.is_some() {
            service.replay_pending_operations()?;
            if let Some(binding) = service.binding()? {
                if binding.root_path.is_dir() {
                    service.restart_watcher(&binding.root_path)?;
                } else {
                    service.mode = LibraryMode::ReadOnly;
                    service.read_only_reason = Some("root_unavailable".to_owned());
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
        let manifest = inventory_manifest(&scan(&root_path)?);
        let now = now_millis();
        self.database_mut()?.connection_mut().execute(
            "INSERT INTO libraries (id, root_path, root_identity, binding_generation, previous_root_path, manifest_json, updated_at) VALUES (?1, ?2, ?3, 1, NULL, ?4, ?5)",
            params![library_id, path_text(&root_path), root_identity, manifest, now],
        ).map_err(LibraryError::database)?;
        self.mode = LibraryMode::Writable;
        self.read_only_reason = None;
        self.reconcile()?;
        self.restart_watcher(&root_path)?;
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
        Ok(RelinkPreview {
            candidate_path: probe.candidate_path,
            library_id: binding.library_id,
            expected_generation: binding.generation,
            expected_state_token: state_token(&snapshot),
            root_identity: probe.root_identity.unwrap_or_default(),
            matched_projects,
            matched_documents,
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
            || state_token(&self.snapshot()?) != preview.expected_state_token
        {
            return Err(LibraryError::new(
                "stale_relink",
                "The library changed after the relink preview",
            ));
        }
        let fresh = self.preview_relink(&preview.candidate_path)?;
        if fresh.root_identity != preview.root_identity
            || fresh.matched_projects != preview.matched_projects
            || fresh.matched_documents != preview.matched_documents
        {
            return Err(LibraryError::new(
                "stale_relink",
                "The candidate changed after the relink preview",
            ));
        }
        let manifest = inventory_manifest(&scan(&preview.candidate_path)?);
        self.database_mut()?.connection_mut().execute(
            "UPDATE libraries SET previous_root_path = root_path, root_path = ?1, root_identity = ?2, binding_generation = binding_generation + 1, manifest_json = ?3, updated_at = ?4 WHERE id = ?5",
            params![path_text(&preview.candidate_path), preview.root_identity, manifest, now_millis(), preview.library_id],
        ).map_err(LibraryError::database)?;
        self.mode = LibraryMode::Writable;
        self.read_only_reason = None;
        self.restart_watcher(&preview.candidate_path)?;
        self.reconcile()?;
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
        self.require_writable()?;
        let name = validate_project_name(name)?;
        let binding = self.required_binding()?;
        self.ensure_project_path_available(&binding.library_id, &name, None)?;
        let target = resolve_new(&binding.root_path, &name)?;
        fs::create_dir(&target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        let id = Uuid::new_v4().to_string();
        let identity = file_identity(&target)?;
        if let Err(error) = self.database_mut()?.connection_mut().execute(
            "INSERT INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, binding.library_id, name, path_key(&name), identity, now_millis()],
        ) {
            let _ = fs::remove_dir(&target);
            return Err(database_conflict(error));
        }
        self.project_by_id(&id)
    }

    pub fn rename_project(
        &mut self,
        project_id: &str,
        name: &str,
    ) -> Result<ProjectSnapshot, LibraryError> {
        self.require_writable()?;
        let name = validate_project_name(name)?;
        let binding = self.required_binding()?;
        let project = self.project_by_id(project_id)?;
        self.ensure_project_path_available(&binding.library_id, &name, Some(project_id))?;
        let source = resolve_existing(&binding.root_path, &project.relative_path)?;
        let target = binding.root_path.join(&name);
        case_aware_rename(&source, &target, &Uuid::new_v4().to_string())
            .map_err(map_conflict_io)?;
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
        self.project_by_id(project_id)
    }

    pub fn create_document(
        &mut self,
        project_id: &str,
        name: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        self.create_document_through_phase(project_id, name, JournalPhase::CleanupComplete)?
            .ok_or_else(|| LibraryError::new("journal_incomplete", "Document was not committed"))
    }

    pub fn create_document_interrupted_for_test(
        &mut self,
        project_id: &str,
        name: &str,
        stop_after: JournalPhase,
    ) -> Result<(), LibraryError> {
        self.create_document_through_phase(project_id, name, stop_after)?;
        Ok(())
    }

    pub fn rename_document(
        &mut self,
        document_id: &str,
        name: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        self.require_writable()?;
        let name = validate_document_name(name)?;
        let binding = self.required_binding()?;
        let document = self.document_by_id(document_id)?;
        let project = self.project_for_document(document_id)?;
        let relative = document_relative_path(&project.relative_path, &name);
        self.ensure_document_path_available(&binding.library_id, &relative, Some(document_id))?;
        let source = resolve_existing(&binding.root_path, &document.relative_path)?;
        let target = binding
            .root_path
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        case_aware_rename(&source, &target, &Uuid::new_v4().to_string())
            .map_err(map_conflict_io)?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE documents SET relative_path = ?1, path_key = ?2, file_identity = ?3, disk_fingerprint = ?4, updated_at = ?5 WHERE id = ?6",
            params![relative, path_key(&relative), file_identity(&target)?, fingerprint(&target)?, now_millis(), document_id],
        ).map_err(database_conflict)?;
        self.document_by_id(document_id)
    }

    pub fn move_document(
        &mut self,
        document_id: &str,
        target_project_id: &str,
    ) -> Result<DocumentSnapshot, LibraryError> {
        self.require_writable()?;
        let binding = self.required_binding()?;
        let document = self.document_by_id(document_id)?;
        let target_project = self.project_by_id(target_project_id)?;
        let name = Path::new(&document.relative_path)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| LibraryError::invalid_path("Document path is invalid"))?;
        let relative = document_relative_path(&target_project.relative_path, name);
        self.ensure_document_path_available(&binding.library_id, &relative, Some(document_id))?;
        let source = resolve_existing(&binding.root_path, &document.relative_path)?;
        let target = resolve_new(&binding.root_path, &relative)?;
        rename_no_replace(&source, &target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        self.database_mut()?.connection_mut().execute(
            "UPDATE documents SET project_id = ?1, relative_path = ?2, path_key = ?3, file_identity = ?4, updated_at = ?5 WHERE id = ?6",
            params![target_project_id, relative, path_key(&relative), file_identity(&target)?, now_millis(), document_id],
        ).map_err(database_conflict)?;
        self.document_by_id(document_id)
    }

    pub fn delete_document(&mut self, document_id: &str) -> Result<DeletedDocument, LibraryError> {
        self.require_writable()?;
        let binding = self.required_binding()?;
        let document = self.document_by_id(document_id)?;
        let source = resolve_existing(&binding.root_path, &document.relative_path)?;
        let recovery_directory = self
            .app_data_dir
            .join("recoverable-deletions")
            .join(Uuid::new_v4().to_string());
        fs::create_dir_all(&recovery_directory).map_err(LibraryError::io)?;
        let recovery_path = recovery_directory.join(
            source
                .file_name()
                .ok_or_else(|| LibraryError::invalid_path("Document path is invalid"))?,
        );
        rename_no_replace(&source, &recovery_path).map_err(map_conflict_io)?;
        self.database_mut()?
            .connection_mut()
            .execute("DELETE FROM documents WHERE id = ?1", [document_id])
            .map_err(LibraryError::database)?;
        Ok(DeletedDocument { recovery_path })
    }

    pub fn reconcile(&mut self) -> Result<LibrarySnapshot, LibraryError> {
        self.require_writable()?;
        let binding = self.required_binding()?;
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
    ) -> Result<Option<DocumentSnapshot>, LibraryError> {
        self.require_writable()?;
        let name = validate_document_name(name)?;
        let binding = self.required_binding()?;
        let project = self.project_by_id(project_id)?;
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
            &payload,
            &temporary_relative,
        )?;
        if stop_after == JournalPhase::IntentRecorded {
            return Ok(None);
        }
        let temporary = resolve_new(&binding.root_path, &temporary_relative)?;
        write_durable(&temporary, b"")?;
        self.update_operation(
            &operation_id,
            JournalPhase::TemporaryDurable,
            Some("sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        )?;
        if stop_after == JournalPhase::TemporaryDurable {
            return Ok(None);
        }
        let target = resolve_new(&binding.root_path, &relative)?;
        rename_no_replace(&temporary, &target).map_err(map_conflict_io)?;
        sync_parent(&target)?;
        let final_fingerprint = fingerprint(&target)?;
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
                "SELECT id, phase, payload_json, temporary_path, finalized_fingerprint FROM pending_file_operations ORDER BY created_at, id",
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
            let payload: OperationPayload = serde_json::from_str(&payload_json)
                .map_err(|error| LibraryError::new("journal_invalid", error.to_string()))?;
            match (phase, payload) {
                (
                    JournalPhase::FilesystemFinalized | JournalPhase::MetadataCommitted,
                    OperationPayload::CreateDocument {
                        document_id,
                        project_id,
                        relative_path,
                    },
                ) => {
                    let target = resolve_existing(&binding.root_path, &relative_path)?;
                    let actual = fingerprint(&target)?;
                    if expected_fingerprint
                        .as_deref()
                        .is_some_and(|value| value != actual)
                    {
                        return Err(LibraryError::new(
                            "journal_mismatch",
                            "Finalized document fingerprint no longer matches the journal",
                        ));
                    }
                    if phase == JournalPhase::FilesystemFinalized {
                        self.insert_document_metadata(
                            &document_id,
                            &binding.library_id,
                            &project_id,
                            &relative_path,
                            &target,
                            &actual,
                        )?;
                    }
                    self.finish_operation(&id)?;
                }
                (JournalPhase::IntentRecorded | JournalPhase::TemporaryDurable, _) => {
                    if let Some(relative) = temporary_path {
                        if let Ok(path) = resolve_existing(&binding.root_path, &relative) {
                            if expected_fingerprint.as_deref().is_none_or(|value| {
                                fingerprint(&path).ok().as_deref() == Some(value)
                            }) {
                                let _ = fs::remove_file(path);
                            }
                        }
                    }
                    self.finish_operation(&id)?;
                }
                (JournalPhase::CleanupComplete, _) => self.finish_operation(&id)?,
            }
        }
        Ok(())
    }

    fn reconcile_scanned(
        &mut self,
        binding: &LibraryBinding,
        scanned: &[ScannedProject],
    ) -> Result<(), LibraryError> {
        let now = now_millis();
        let transaction = self
            .database_mut()?
            .connection_mut()
            .transaction()
            .map_err(LibraryError::database)?;
        let existing_projects = load_existing_projects(&transaction, &binding.library_id)?;
        let mut seen_projects = Vec::new();
        let mut seen_documents = Vec::new();
        for scanned_project in scanned {
            let project_key = path_key(&scanned_project.relative_path);
            let matched_project = existing_projects
                .iter()
                .find(|project| project.path_key == project_key)
                .or_else(|| unique_project_identity(&existing_projects, scanned_project));
            let project_id = matched_project
                .map(|project| project.id.clone())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            transaction.execute(
                "INSERT INTO projects (id, library_id, relative_path, path_key, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) ON CONFLICT(id) DO UPDATE SET relative_path = excluded.relative_path, path_key = excluded.path_key, file_identity = excluded.file_identity, updated_at = excluded.updated_at",
                params![project_id, binding.library_id, scanned_project.relative_path, project_key, scanned_project.file_identity, now],
            ).map_err(database_conflict)?;
            seen_projects.push(project_id.clone());
            let existing_documents = load_existing_documents(&transaction, &binding.library_id)?;
            for scanned_document in &scanned_project.documents {
                let document_key = path_key(&scanned_document.relative_path);
                let matched_document = existing_documents
                    .iter()
                    .find(|document| document.path_key == document_key)
                    .or_else(|| unique_document_identity(&existing_documents, scanned_document));
                let document_id = matched_document
                    .map(|document| document.id.clone())
                    .unwrap_or_else(|| Uuid::new_v4().to_string());
                transaction.execute(
                    "INSERT INTO documents (id, library_id, project_id, relative_path, path_key, source_path, imported_at, disk_fingerprint, disk_revision, file_identity, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, ?6, 0, ?7, ?8, ?8) ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, relative_path = excluded.relative_path, path_key = excluded.path_key, disk_fingerprint = excluded.disk_fingerprint, file_identity = excluded.file_identity, updated_at = excluded.updated_at",
                    params![document_id, binding.library_id, project_id, scanned_document.relative_path, document_key, scanned_document.fingerprint, scanned_document.file_identity, now],
                ).map_err(database_conflict)?;
                seen_documents.push(document_id);
            }
        }
        delete_unseen(
            &transaction,
            "documents",
            &binding.library_id,
            &seen_documents,
        )?;
        delete_unseen(
            &transaction,
            "projects",
            &binding.library_id,
            &seen_projects,
        )?;
        let manifest = inventory_manifest(scanned);
        transaction
            .execute(
                "UPDATE libraries SET manifest_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![manifest, now, binding.library_id],
            )
            .map_err(LibraryError::database)?;
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
        payload: &OperationPayload,
        temporary_path: &str,
    ) -> Result<(), LibraryError> {
        let payload = serde_json::to_string(payload)
            .map_err(|error| LibraryError::new("journal_invalid", error.to_string()))?;
        self.database_mut()?.connection_mut().execute(
            "INSERT INTO pending_file_operations (id, library_id, kind, phase, payload_json, expected_fingerprint, finalized_fingerprint, temporary_path, created_at, updated_at) VALUES (?1, ?2, 'create_document', ?3, ?4, NULL, NULL, ?5, ?6, ?6)",
            params![id, library_id, JournalPhase::IntentRecorded.as_str(), payload, temporary_path, now_millis()],
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
            "UPDATE pending_file_operations SET phase = ?1, expected_fingerprint = COALESCE(?2, expected_fingerprint), finalized_fingerprint = CASE WHEN ?1 = 'Filesystem finalized' THEN ?2 ELSE finalized_fingerprint END, updated_at = ?3 WHERE id = ?4",
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
        .prepare("SELECT id, path_key, file_identity FROM documents WHERE library_id = ?1")
        .map_err(LibraryError::database)?;
    let rows = statement
        .query_map([library_id], |row| {
            Ok(ExistingDocument {
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

fn unique_project_identity<'a>(
    existing: &'a [ExistingProject],
    scanned: &ScannedProject,
) -> Option<&'a ExistingProject> {
    let identity = scanned.file_identity.as_ref()?;
    let mut matches = existing
        .iter()
        .filter(|project| project.file_identity.as_ref() == Some(identity));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn unique_document_identity<'a>(
    existing: &'a [ExistingDocument],
    scanned: &ScannedDocument,
) -> Option<&'a ExistingDocument> {
    let identity = scanned.file_identity.as_ref()?;
    let mut matches = existing
        .iter()
        .filter(|document| document.file_identity.as_ref() == Some(identity));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn delete_unseen(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    library_id: &str,
    seen_ids: &[String],
) -> Result<(), LibraryError> {
    let query = format!("SELECT id FROM {table} WHERE library_id = ?1");
    let mut statement = transaction
        .prepare(&query)
        .map_err(LibraryError::database)?;
    let existing = statement
        .query_map([library_id], |row| row.get::<_, String>(0))
        .map_err(LibraryError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(LibraryError::database)?;
    drop(statement);
    let delete = format!("DELETE FROM {table} WHERE id = ?1");
    for id in existing {
        if !seen_ids.contains(&id) {
            transaction
                .execute(&delete, [&id])
                .map_err(LibraryError::database)?;
        }
    }
    Ok(())
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
