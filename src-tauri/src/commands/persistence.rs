use tauri::State;

use crate::commands::library::{service, LibraryState};
use crate::domain::library::{DocumentContent, DocumentSnapshot, LibraryError};
use crate::domain::recovery::{RecoverySnapshot, RecoverySnapshotRequest, SaveDocumentResult};

#[cfg(debug_assertions)]
fn webdriver_test_path(variable: &str) -> Option<std::path::PathBuf> {
    if std::env::var("CAIRN_WEBDRIVER_MODE").as_deref() != Ok("1") {
        return None;
    }
    std::env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

#[cfg(debug_assertions)]
fn mark_webdriver_recovery(snapshot: &RecoverySnapshot) -> Result<(), LibraryError> {
    let Some(path) = webdriver_test_path("CAIRN_WEBDRIVER_RECOVERY_MARKER_PATH") else {
        return Ok(());
    };
    std::fs::write(path, snapshot.content_hash.as_bytes()).map_err(LibraryError::io)
}

#[cfg(debug_assertions)]
fn prevent_webdriver_disk_save() -> Result<(), LibraryError> {
    let Some(path) = webdriver_test_path("CAIRN_WEBDRIVER_DISK_SAVE_BLOCK_PATH") else {
        return Ok(());
    };
    std::fs::write(path, b"disk-save-blocked").map_err(LibraryError::io)?;
    Err(LibraryError::new(
        "webdriver_disk_save_blocked",
        "The WebDriver recovery test blocked the disk save",
    ))
}

#[cfg(not(debug_assertions))]
fn prevent_webdriver_disk_save() -> Result<(), LibraryError> {
    Ok(())
}

#[tauri::command]
pub fn store_recovery_snapshot(
    state: State<'_, LibraryState>,
    request: RecoverySnapshotRequest,
) -> Result<RecoverySnapshot, LibraryError> {
    let snapshot = service(&state)?.store_recovery_snapshot(request)?;
    #[cfg(debug_assertions)]
    mark_webdriver_recovery(&snapshot)?;
    Ok(snapshot)
}

#[tauri::command]
pub fn load_recovery_snapshot(
    state: State<'_, LibraryState>,
    document_id: String,
) -> Result<Option<RecoverySnapshot>, LibraryError> {
    service(&state)?.load_recovery_snapshot(&document_id)
}

#[tauri::command]
pub fn discard_recovery_snapshot(
    state: State<'_, LibraryState>,
    document_id: String,
    session_generation: String,
) -> Result<bool, LibraryError> {
    service(&state)?.discard_recovery_snapshot(&document_id, &session_generation)
}

#[tauri::command]
pub fn save_document(
    state: State<'_, LibraryState>,
    request: RecoverySnapshotRequest,
) -> Result<SaveDocumentResult, LibraryError> {
    prevent_webdriver_disk_save()?;
    service(&state)?.save_document(request)
}

#[tauri::command]
pub fn save_recovery_copy(
    state: State<'_, LibraryState>,
    document_id: String,
    session_generation: String,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.save_recovery_copy(&document_id, &session_generation)
}

#[tauri::command]
pub fn reload_document_from_disk(
    state: State<'_, LibraryState>,
    document_id: String,
    session_generation: Option<String>,
) -> Result<DocumentContent, LibraryError> {
    service(&state)?.reload_document_from_disk(&document_id, session_generation.as_deref())
}
