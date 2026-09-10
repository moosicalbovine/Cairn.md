use tauri::State;

use crate::commands::library::{service, LibraryState};
use crate::domain::import::{ImportSource, TrackedFolderEntry, TrackedFolderSnapshot};
use crate::domain::library::{DocumentSnapshot, LibraryError};

#[tauri::command]
pub async fn import_markdown(
    state: State<'_, LibraryState>,
    project_id: String,
    source: ImportSource,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.import_document(&project_id, source)
}

#[tauri::command]
pub fn add_tracked_folder(
    state: State<'_, LibraryState>,
    path: String,
) -> Result<TrackedFolderSnapshot, LibraryError> {
    service(&state)?.add_tracked_folder(path.as_ref())
}

#[tauri::command]
pub fn list_tracked_folders(
    state: State<'_, LibraryState>,
) -> Result<Vec<TrackedFolderSnapshot>, LibraryError> {
    service(&state)?.list_tracked_folders()
}

#[tauri::command]
pub fn remove_tracked_folder(
    state: State<'_, LibraryState>,
    id: String,
) -> Result<(), LibraryError> {
    service(&state)?.remove_tracked_folder(&id)
}

#[tauri::command]
pub fn list_tracked_folder_entries(
    state: State<'_, LibraryState>,
    folder_id: String,
    relative_directory: Option<String>,
) -> Result<Vec<TrackedFolderEntry>, LibraryError> {
    service(&state)?.list_tracked_folder_entries(&folder_id, relative_directory.as_deref())
}
