use tauri::State;

use crate::commands::library::{service, LibraryState};
use crate::domain::library::{DocumentContent, DocumentSnapshot, LibraryError};
use crate::domain::recovery::{RecoverySnapshot, RecoverySnapshotRequest, SaveDocumentResult};

#[cfg(debug_assertions)]
fn wait_for_webdriver_save_barrier() -> Result<(), LibraryError> {
    if std::env::var("CAIRN_WEBDRIVER_MODE").as_deref() != Ok("1") {
        return Ok(());
    }
    let Some(path) = std::env::var_os("CAIRN_WEBDRIVER_SAVE_BARRIER_PATH")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
    else {
        return Ok(());
    };
    std::fs::write(&path, b"recovery-durable").map_err(LibraryError::io)?;
    let started = std::time::Instant::now();
    while path.exists() {
        if started.elapsed() >= std::time::Duration::from_secs(60) {
            return Err(LibraryError::new(
                "webdriver_barrier_timeout",
                "The WebDriver save barrier was not released",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    Ok(())
}

#[tauri::command]
pub fn store_recovery_snapshot(
    state: State<'_, LibraryState>,
    request: RecoverySnapshotRequest,
) -> Result<RecoverySnapshot, LibraryError> {
    service(&state)?.store_recovery_snapshot(request)
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
    #[cfg(debug_assertions)]
    wait_for_webdriver_save_barrier()?;
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
