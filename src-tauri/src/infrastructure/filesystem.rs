use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(not(any(windows, unix)))]
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::library::LibraryError;
use crate::domain::project::validate_relative_path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRootProbe {
    pub candidate_path: PathBuf,
    pub root_identity: Option<String>,
    pub exists: bool,
    pub is_directory: bool,
    pub can_create: bool,
    pub can_flush: bool,
    pub can_rename_without_overwrite: bool,
    pub recoverable_delete: bool,
    pub can_bind: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScannedProject {
    pub relative_path: String,
    pub file_identity: Option<String>,
    pub documents: Vec<ScannedDocument>,
}

#[derive(Debug, Clone)]
pub struct ScannedDocument {
    pub relative_path: String,
    pub file_identity: Option<String>,
    pub fingerprint: String,
}

pub fn probe_candidate(candidate: &Path) -> Result<CandidateRootProbe, LibraryError> {
    let candidate_path = candidate.to_path_buf();
    if !candidate.exists() {
        return Ok(failed_probe(candidate_path, "root_unavailable", false));
    }
    if !candidate.is_dir() {
        return Ok(failed_probe(candidate_path, "root_not_directory", true));
    }
    let canonical = match fs::canonicalize(candidate) {
        Ok(path) => path,
        Err(_) => return Ok(failed_probe(candidate_path, "root_unavailable", true)),
    };
    if is_link_or_reparse(candidate)? {
        return Ok(failed_probe(candidate_path, "root_reparse_point", true));
    }

    let token = Uuid::new_v4();
    let source = canonical.join(format!(".cairn-probe-{token}.tmp"));
    let target = canonical.join(format!(".cairn-probe-{token}.moved"));
    let collision = canonical.join(format!(".cairn-probe-{token}.collision"));
    let mut can_create = false;
    let mut can_flush = false;
    let mut can_rename = false;
    let mut can_recover = false;

    let result = (|| -> Result<(), LibraryError> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&source)
            .map_err(LibraryError::io)?;
        can_create = true;
        file.write_all(b"cairn capability probe")
            .map_err(LibraryError::io)?;
        file.sync_all().map_err(LibraryError::io)?;
        can_flush = true;

        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&collision)
            .map_err(LibraryError::io)?
            .sync_all()
            .map_err(LibraryError::io)?;
        if OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&collision)
            .is_err()
        {
            rename_no_replace(&source, &target).map_err(LibraryError::io)?;
            can_rename = true;
        }
        fs::rename(&target, &source).map_err(LibraryError::io)?;
        let recycled = recycle_file(&source)?;
        can_recover = !source.exists();
        if let Some(path) = recycled {
            let _ = fs::remove_file(path);
        }
        Ok(())
    })();

    let _ = fs::remove_file(&source);
    let _ = fs::remove_file(&target);
    let _ = fs::remove_file(&collision);

    let reason = result
        .err()
        .map(|error| format!("capability_probe_failed:{error}"));
    let can_bind = can_create && can_flush && can_rename && can_recover && reason.is_none();
    Ok(CandidateRootProbe {
        candidate_path: canonical.clone(),
        root_identity: file_identity(&canonical)?,
        exists: true,
        is_directory: true,
        can_create,
        can_flush,
        can_rename_without_overwrite: can_rename,
        recoverable_delete: can_recover,
        can_bind,
        reason,
    })
}

fn failed_probe(path: PathBuf, reason: &str, exists: bool) -> CandidateRootProbe {
    CandidateRootProbe {
        candidate_path: path,
        root_identity: None,
        exists,
        is_directory: false,
        can_create: false,
        can_flush: false,
        can_rename_without_overwrite: false,
        recoverable_delete: false,
        can_bind: false,
        reason: Some(reason.to_owned()),
    }
}

pub fn scan(root: &Path) -> Result<Vec<ScannedProject>, LibraryError> {
    let canonical_root = canonical_root(root)?;
    let mut projects = Vec::new();
    for entry in fs::read_dir(&canonical_root).map_err(LibraryError::io)? {
        let entry = entry.map_err(LibraryError::io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.eq_ignore_ascii_case(".git") {
            continue;
        }
        let metadata = entry.metadata().map_err(LibraryError::io)?;
        if !metadata.is_dir() || is_link_or_reparse(&entry.path())? {
            continue;
        }
        let canonical_project = fs::canonicalize(entry.path()).map_err(LibraryError::io)?;
        ensure_below(&canonical_root, &canonical_project)?;
        let mut documents = Vec::new();
        for document in fs::read_dir(&canonical_project).map_err(LibraryError::io)? {
            let document = document.map_err(LibraryError::io)?;
            let document_name = document.file_name().to_string_lossy().into_owned();
            if document_name.starts_with('.') || is_link_or_reparse(&document.path())? {
                continue;
            }
            let metadata = document.metadata().map_err(LibraryError::io)?;
            let is_markdown = Path::new(&document_name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
            if !metadata.is_file() || !is_markdown {
                continue;
            }
            let canonical_document = fs::canonicalize(document.path()).map_err(LibraryError::io)?;
            ensure_below(&canonical_root, &canonical_document)?;
            documents.push(ScannedDocument {
                relative_path: format!("{name}/{document_name}"),
                file_identity: file_identity(&canonical_document)?,
                fingerprint: fingerprint(&canonical_document)?,
            });
        }
        documents.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        projects.push(ScannedProject {
            relative_path: name,
            file_identity: file_identity(&canonical_project)?,
            documents,
        });
    }
    projects.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(projects)
}

pub fn resolve_existing(root: &Path, relative_path: &str) -> Result<PathBuf, LibraryError> {
    validate_managed_relative_path(relative_path)?;
    let canonical_root = canonical_root(root)?;
    let candidate = root.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    reject_link_components(root, &candidate)?;
    let canonical = fs::canonicalize(candidate).map_err(LibraryError::io)?;
    ensure_below(&canonical_root, &canonical)?;
    Ok(canonical)
}

pub fn resolve_new(root: &Path, relative_path: &str) -> Result<PathBuf, LibraryError> {
    validate_managed_relative_path(relative_path)?;
    let canonical_root = canonical_root(root)?;
    let candidate = root.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let parent = candidate
        .parent()
        .ok_or_else(|| LibraryError::invalid_path("Mutation target has no parent"))?;
    reject_link_components(root, parent)?;
    let canonical_parent = fs::canonicalize(parent).map_err(LibraryError::io)?;
    if canonical_parent != canonical_root {
        ensure_below(&canonical_root, &canonical_parent)?;
    }
    Ok(candidate)
}

fn validate_managed_relative_path(relative_path: &str) -> Result<(), LibraryError> {
    let component_count = relative_path.split('/').count();
    if !(1..=2).contains(&component_count) {
        return Err(LibraryError::invalid_path(
            "Managed paths must contain one project and at most one document",
        ));
    }
    validate_relative_path(relative_path, component_count).map(|_| ())
}

pub fn rename_no_replace(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "target exists",
        ));
    }
    if source.is_file() {
        fs::hard_link(source, target)?;
        if let Err(error) = fs::remove_file(source) {
            let _ = fs::remove_file(target);
            return Err(error);
        }
        Ok(())
    } else {
        fs::rename(source, target)
    }
}

pub fn case_aware_rename(source: &Path, target: &Path, operation_id: &str) -> std::io::Result<()> {
    if source == target {
        return Ok(());
    }
    let source_folded = source.to_string_lossy().to_lowercase();
    let target_folded = target.to_string_lossy().to_lowercase();
    if source_folded == target_folded {
        let temporary = source.with_file_name(format!(".cairn-case-{operation_id}.tmp"));
        rename_no_replace(source, &temporary)?;
        if let Err(error) = rename_no_replace(&temporary, target) {
            let _ = rename_no_replace(&temporary, source);
            return Err(error);
        }
        return Ok(());
    }
    rename_no_replace(source, target)
}

pub fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), LibraryError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(LibraryError::io)?;
    file.write_all(bytes).map_err(LibraryError::io)?;
    file.sync_all().map_err(LibraryError::io)
}

#[cfg(windows)]
pub fn recycle_file(path: &Path) -> Result<Option<PathBuf>, LibraryError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{
        SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE,
        SHFILEOPSTRUCTW,
    };

    let mut from = path.as_os_str().encode_wide().collect::<Vec<_>>();
    from.push(0);
    from.push(0);
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
        ..Default::default()
    };
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 || operation.fAnyOperationsAborted != 0 {
        return Err(LibraryError::new(
            "recycle_failed",
            format!("Windows Recycle Bin operation failed ({result})"),
        ));
    }
    Ok(None)
}

#[cfg(not(windows))]
pub fn recycle_file(path: &Path) -> Result<Option<PathBuf>, LibraryError> {
    let recovery = path.with_file_name(format!(".cairn-test-trash-{}", Uuid::new_v4()));
    fs::rename(path, &recovery).map_err(LibraryError::io)?;
    Ok(Some(recovery))
}

pub fn fingerprint(path: &Path) -> Result<String, LibraryError> {
    let bytes = fs::read(path).map_err(LibraryError::io)?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub fn file_identity(path: &Path) -> Result<Option<String>, LibraryError> {
    #[cfg(windows)]
    {
        let handle = winapi_util::Handle::from_path_any(path).map_err(LibraryError::io)?;
        let information = winapi_util::file::information(&handle).map_err(LibraryError::io)?;
        Ok(Some(format!(
            "win:{:x}:{:x}",
            information.volume_serial_number(),
            information.file_index()
        )))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).map_err(LibraryError::io)?;
        Ok(Some(format!(
            "unix:{:x}:{:x}",
            metadata.dev(),
            metadata.ino()
        )))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let metadata = fs::metadata(path).map_err(LibraryError::io)?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        Ok(Some(format!("fallback:{}:{modified}", metadata.len())))
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf, LibraryError> {
    let canonical = fs::canonicalize(root).map_err(LibraryError::io)?;
    if !canonical.is_dir() || is_link_or_reparse(root)? {
        return Err(LibraryError::path_escape(
            "Library root is not a safe directory",
        ));
    }
    Ok(canonical)
}

fn ensure_below(root: &Path, candidate: &Path) -> Result<(), LibraryError> {
    if candidate == root || !candidate.starts_with(root) {
        return Err(LibraryError::path_escape(
            "Resolved path escapes the active library root",
        ));
    }
    Ok(())
}

fn reject_link_components(root: &Path, target: &Path) -> Result<(), LibraryError> {
    let mut current = root.to_path_buf();
    let relative = target
        .strip_prefix(root)
        .map_err(|_| LibraryError::path_escape("Target is outside the library root"))?;
    for component in relative.components() {
        current.push(component);
        if current.exists() && is_link_or_reparse(&current)? {
            return Err(LibraryError::path_escape(
                "Mutation target crosses a link or reparse point",
            ));
        }
    }
    Ok(())
}

fn is_link_or_reparse(path: &Path) -> Result<bool, LibraryError> {
    let metadata = fs::symlink_metadata(path).map_err(LibraryError::io)?;
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

pub fn sync_parent(path: &Path) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        if let Ok(directory) = File::open(parent) {
            directory.sync_all().map_err(LibraryError::io)?;
        }
    }
    Ok(())
}
