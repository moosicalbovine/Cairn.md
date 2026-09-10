use tauri::State;

use crate::commands::library::{service, LibraryState};
use crate::domain::import::ImportSource;
use crate::domain::library::{DocumentSnapshot, LibraryError};

#[tauri::command]
pub async fn import_markdown(
    state: State<'_, LibraryState>,
    project_id: String,
    source: ImportSource,
) -> Result<DocumentSnapshot, LibraryError> {
    service(&state)?.import_document(&project_id, source)
}
