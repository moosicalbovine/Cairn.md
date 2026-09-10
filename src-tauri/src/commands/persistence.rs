use tauri::State;

use crate::commands::library::{service, LibraryState};
use crate::domain::library::LibraryError;
use crate::domain::recovery::{RecoverySnapshot, RecoverySnapshotRequest};

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
