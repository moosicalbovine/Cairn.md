use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::library::LibraryError;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ImportSource {
    ExternalPath {
        #[serde(rename = "absolutePath")]
        absolute_path: PathBuf,
    },
    TrackedFile {
        #[serde(rename = "trackedFolderId")]
        tracked_folder_id: String,
        #[serde(rename = "relativePath")]
        relative_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFolderSnapshot {
    pub id: String,
    pub absolute_path: PathBuf,
    pub display_name: String,
    pub available: bool,
    pub last_scan_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedFolderEntry {
    pub relative_path: String,
    pub display_name: String,
    pub is_directory: bool,
}

pub fn collision_name(original: &str, sequence: usize) -> Result<String, LibraryError> {
    let path = Path::new(original);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LibraryError::invalid_path("Source filename is invalid"))?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| LibraryError::invalid_path("Source extension is invalid"))?;
    if sequence == 1 {
        return Ok(original.to_owned());
    }
    Ok(format!("{stem} ({sequence}).{extension}"))
}

pub fn validate_tracked_relative_path(relative_path: &str) -> Result<PathBuf, LibraryError> {
    if relative_path.is_empty()
        || relative_path.contains('\\')
        || relative_path.starts_with('/')
        || relative_path.as_bytes().get(1) == Some(&b':')
    {
        return Err(LibraryError::invalid_path(
            "Tracked paths must be relative and use forward slashes",
        ));
    }
    let path = PathBuf::from(relative_path);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(LibraryError::invalid_path(
            "Tracked path contains an invalid component",
        ));
    }
    Ok(path)
}
