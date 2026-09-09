use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use tauri::State;

use crate::domain::library::{
    DeletedDocument, DocumentSnapshot, LibraryError, LibraryService, LibrarySnapshot,
    ProjectSnapshot, RelinkPreview,
};
use crate::infrastructure::filesystem::CandidateRootProbe;

pub type LibraryState = Mutex<LibraryService>;

fn service<'a>(
    state: &'a State<'_, LibraryState>,
) -> Result<MutexGuard<'a, LibraryService>, LibraryError> {
    state
        .lock()
        .map_err(|_| LibraryError::new("library_unavailable", "Library state lock was poisoned"))
}

#[tauri::command]
pub fn probe_library_root(
    state: State<'_, LibraryState>,
    candidate_path: String,
) -> Result<CandidateRootProbe, LibraryError> {
    service(&state)?.probe_candidate(&PathBuf::from(candidate_path))
}

#[tauri::command]
pub fn bind_library_root(
    state: State<'_, LibraryState>,
    root_path: String,
) -> Result<LibrarySnapshot, LibraryError> {
    service(&state)?.bind_root(&PathBuf::from(root_path))
}

#[tauri::command]
pub fn preview_library_relink(
    state: State<'_, LibraryState>,
    candidate_path: String,
) -> Result<RelinkPreview, LibraryError> {
    service(&state)?.preview_relink(&PathBuf::from(candidate_path))
}

#[tauri::command]
pub fn confirm_library_relink(
    state: State<'_, LibraryState>,
    preview: RelinkPreview,
) -> Result<LibrarySnapshot, LibraryError> {
    service(&state)?.confirm_relink(preview)
}

#[tauri::command]
pub fn library_snapshot(state: State<'_, LibraryState>) -> Result<LibrarySnapshot, LibraryError> {
    service(&state)?.snapshot()
}

#[tauri::command]
pub fn reconcile_library(state: State<'_, LibraryState>) -> Result<LibrarySnapshot, LibraryError> {
    service(&state)?.reconcile()
}

#[tauri::command]
pub fn reconcile_library_if_requested(
    state: State<'_, LibraryState>,
) -> Result<Option<LibrarySnapshot>, LibraryError> {
    service(&state)?.reconcile_if_requested()
}

#[tauri::command]
pub fn create_project(
    state: State<'_, LibraryState>,
    name: String,
) -> Result<ProjectSnapshot, LibraryError> {
    service(&state)?.create_project(&name)
}

#[tauri::command]
pub fn rename_project(
    state: State<'_, LibraryState>,
    project_id: String,
    name: String,
) -> Result<ProjectSnapshot, LibraryError> {
    service(&state)?.rename_project(&project_id, &name)
}

#[tauri::command]
pub fn create_document(
    state: State<'_, LibraryState>,
    project_id: String,
    name: String,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.create_document(&project_id, &name)
}

#[tauri::command]
pub fn rename_document(
    state: State<'_, LibraryState>,
    document_id: String,
    name: String,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.rename_document(&document_id, &name)
}

#[tauri::command]
pub fn move_document(
    state: State<'_, LibraryState>,
    document_id: String,
    target_project_id: String,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.move_document(&document_id, &target_project_id)
}

#[tauri::command]
pub fn delete_document(
    state: State<'_, LibraryState>,
    document_id: String,
) -> Result<DeletedDocument, LibraryError> {
    service(&state)?.delete_document(&document_id)
}
