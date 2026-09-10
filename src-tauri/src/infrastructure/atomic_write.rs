#[cfg(not(windows))]
use std::fs;
use std::fs::File;
#[cfg(windows)]
use std::fs::OpenOptions;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::domain::library::LibraryError;
#[cfg(not(windows))]
use crate::infrastructure::filesystem::copy_durable_no_replace;
use crate::infrastructure::filesystem::{file_identity, fingerprint, sync_parent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtomicReplaceResult {
    pub fingerprint: String,
    pub file_identity: Option<String>,
    pub backup_path: PathBuf,
}

pub fn replace_if_unchanged(
    target: &Path,
    replacement: &Path,
    backup: &Path,
    expected_fingerprint: &str,
    expected_identity: Option<&str>,
    intended_fingerprint: &str,
) -> Result<AtomicReplaceResult, LibraryError> {
    if backup.exists() {
        return Err(LibraryError::new(
            "owned_artifact_conflict",
            "The save backup path is already occupied",
        ));
    }
    if fingerprint(replacement)? != intended_fingerprint {
        return Err(LibraryError::new(
            "temporary_mismatch",
            "The durable save temporary does not contain the intended bytes",
        ));
    }

    replace_platform(
        target,
        replacement,
        backup,
        expected_fingerprint,
        expected_identity,
    )?;
    sync_parent(target)?;

    let actual_fingerprint = fingerprint(target)?;
    if actual_fingerprint != intended_fingerprint {
        return Err(LibraryError::new(
            "save_verification_failed",
            "The saved file does not contain the intended bytes",
        ));
    }

    Ok(AtomicReplaceResult {
        fingerprint: actual_fingerprint,
        file_identity: file_identity(target)?,
        backup_path: backup.to_path_buf(),
    })
}

fn fingerprint_reader(reader: &mut File) -> Result<String, LibraryError> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(LibraryError::io)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn external_change(message: &str) -> LibraryError {
    LibraryError::new("external_change", message)
}

#[cfg(windows)]
fn replace_platform(
    target: &Path,
    replacement: &Path,
    backup: &Path,
    expected_fingerprint: &str,
    expected_identity: Option<&str>,
) -> Result<(), LibraryError> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, ReplaceFileW, BY_HANDLE_FILE_INFORMATION, FILE_SHARE_DELETE,
        FILE_SHARE_READ, REPLACEFILE_WRITE_THROUGH,
    };

    let mut held = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_DELETE)
        .open(target)
        .map_err(|error| {
            external_change(&format!("The document could not be protected: {error}"))
        })?;
    if fingerprint_reader(&mut held)? != expected_fingerprint {
        return Err(external_change(
            "The document changed outside Cairn.md before it could be saved",
        ));
    }

    if let Some(expected) = expected_identity {
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        let read =
            unsafe { GetFileInformationByHandle(held.as_raw_handle() as _, &mut information) };
        if read == 0 {
            return Err(LibraryError::io(std::io::Error::last_os_error()));
        }
        let file_index =
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
        let actual = format!(
            "win:{:x}:{:x}",
            information.dwVolumeSerialNumber, file_index
        );
        if actual != expected {
            return Err(external_change(
                "The document was replaced outside Cairn.md before it could be saved",
            ));
        }
        if file_identity(target)?.as_deref() != Some(expected) {
            return Err(external_change(
                "The document path changed outside Cairn.md before it could be saved",
            ));
        }
    }

    let mut target_wide = target.as_os_str().encode_wide().collect::<Vec<_>>();
    target_wide.push(0);
    let mut replacement_wide = replacement.as_os_str().encode_wide().collect::<Vec<_>>();
    replacement_wide.push(0);
    let mut backup_wide = backup.as_os_str().encode_wide().collect::<Vec<_>>();
    backup_wide.push(0);
    let replaced = unsafe {
        ReplaceFileW(
            target_wide.as_ptr(),
            replacement_wide.as_ptr(),
            backup_wide.as_ptr(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        return Err(LibraryError::io(std::io::Error::last_os_error()));
    }
    drop(held);
    Ok(())
}

#[cfg(not(windows))]
fn replace_platform(
    target: &Path,
    replacement: &Path,
    backup: &Path,
    expected_fingerprint: &str,
    expected_identity: Option<&str>,
) -> Result<(), LibraryError> {
    let mut held = File::open(target).map_err(LibraryError::io)?;
    if fingerprint_reader(&mut held)? != expected_fingerprint
        || expected_identity.is_some_and(|expected| {
            file_identity(target).ok().flatten().as_deref() != Some(expected)
        })
    {
        return Err(external_change(
            "The document changed outside Cairn.md before it could be saved",
        ));
    }
    copy_durable_no_replace(target, backup)?;
    if fingerprint(target)? != expected_fingerprint {
        return Err(external_change(
            "The document changed outside Cairn.md during save",
        ));
    }
    fs::rename(replacement, target).map_err(LibraryError::io)
}
