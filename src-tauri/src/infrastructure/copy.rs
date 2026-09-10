use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::library::LibraryError;

use super::filesystem::{file_identity, fingerprint};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDescriptor {
    pub provenance_path: String,
    pub resolved_path: PathBuf,
    pub identity: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableCopy {
    pub identity: String,
    pub fingerprint: String,
}

pub fn inspect_markdown_source(path: &Path) -> Result<SourceDescriptor, LibraryError> {
    if !path.is_absolute() {
        return Err(LibraryError::new(
            "source_unavailable",
            "Import sources must use an absolute path",
        ));
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !extension.eq_ignore_ascii_case("md") {
        return Err(LibraryError::new(
            "source_not_markdown",
            "Only Markdown files with a .md extension can be imported",
        ));
    }
    let provenance_path = path
        .to_str()
        .ok_or_else(|| LibraryError::new("source_unavailable", "Source path is not Unicode"))?
        .to_owned();
    let resolved_path = fs::canonicalize(path).map_err(source_io_error)?;
    if !resolved_path.is_file() {
        return Err(LibraryError::new(
            "source_not_file",
            "Import source is not a regular file",
        ));
    }
    let identity = file_identity(&resolved_path)
        .map_err(source_error)?
        .ok_or_else(|| LibraryError::new("source_unavailable", "Source identity unavailable"))?;
    let fingerprint = fingerprint(&resolved_path).map_err(source_error)?;
    Ok(SourceDescriptor {
        provenance_path,
        resolved_path,
        identity,
        fingerprint,
    })
}

pub fn copy_stable_source(
    source: &SourceDescriptor,
    target: &Path,
) -> Result<DurableCopy, LibraryError> {
    verify_source(source)?;
    let mut input = File::open(&source.resolved_path).map_err(source_io_error)?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)
        .map_err(destination_io_error)?;
    let target_identity = file_identity(target)
        .map_err(destination_error)?
        .ok_or_else(|| LibraryError::new("destination_busy", "Copy identity unavailable"))?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        drop(output);
        remove_owned_file(target, &target_identity);
        return Err(destination_io_error(error));
    }
    if let Err(error) = output.sync_all() {
        drop(output);
        remove_owned_file(target, &target_identity);
        return Err(destination_io_error(error));
    }
    drop(output);
    let copied_fingerprint = fingerprint(target).map_err(destination_error)?;
    if copied_fingerprint != source.fingerprint {
        remove_owned_file(target, &target_identity);
        return Err(LibraryError::new(
            "source_changed",
            "Source changed while it was being copied",
        ));
    }
    if let Err(error) = verify_source(source) {
        remove_owned_file(target, &target_identity);
        return Err(error);
    }
    Ok(DurableCopy {
        identity: target_identity,
        fingerprint: copied_fingerprint,
    })
}

fn verify_source(source: &SourceDescriptor) -> Result<(), LibraryError> {
    let identity = file_identity(&source.resolved_path).map_err(source_error)?;
    let current_fingerprint = fingerprint(&source.resolved_path).map_err(source_error)?;
    if identity.as_deref() != Some(source.identity.as_str())
        || current_fingerprint != source.fingerprint
    {
        return Err(LibraryError::new(
            "source_changed",
            "Source changed while it was being imported",
        ));
    }
    Ok(())
}

pub(crate) fn remove_owned_file(path: &Path, expected_identity: &str) {
    if file_identity(path).ok().flatten().as_deref() == Some(expected_identity) {
        let _ = fs::remove_file(path);
    }
}

fn source_io_error(error: io::Error) -> LibraryError {
    LibraryError::new("source_unreadable", error.to_string())
}

fn source_error(error: LibraryError) -> LibraryError {
    LibraryError::new("source_unreadable", error.to_string())
}

fn destination_io_error(error: io::Error) -> LibraryError {
    if error.raw_os_error() == Some(112) {
        return LibraryError::new("storage_full", error.to_string());
    }
    if error.kind() == io::ErrorKind::AlreadyExists {
        return LibraryError::new("destination_busy", error.to_string());
    }
    LibraryError::new("destination_unavailable", error.to_string())
}

fn destination_error(error: LibraryError) -> LibraryError {
    LibraryError::new("destination_unavailable", error.to_string())
}
