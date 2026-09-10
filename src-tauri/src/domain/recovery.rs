use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::library::LibraryError;

const MAX_RECOVERY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySnapshotRequest {
    pub document_id: String,
    pub session_generation: String,
    pub revision: i64,
    pub bytes: Vec<u8>,
    pub base_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RecoveryLifecycle {
    Draft,
    Saving,
    Recovered,
    Conflict,
}

impl RecoveryLifecycle {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Saving => "Saving",
            Self::Recovered => "Recovered",
            Self::Conflict => "Conflict",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, LibraryError> {
        match value {
            "Draft" => Ok(Self::Draft),
            "Saving" => Ok(Self::Saving),
            "Recovered" => Ok(Self::Recovered),
            "Conflict" => Ok(Self::Conflict),
            _ => Err(LibraryError::new(
                "recovery_invalid",
                "Unknown recovery lifecycle state",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySnapshot {
    pub document_id: String,
    pub session_generation: String,
    pub revision: i64,
    pub bytes: Vec<u8>,
    pub content_hash: String,
    pub base_fingerprint: String,
    pub intended_disk_hash: String,
    pub operation_id: Option<String>,
    pub lifecycle_state: RecoveryLifecycle,
    pub durable_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SaveStatus {
    Saved,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDocumentResult {
    pub status: SaveStatus,
    pub revision: i64,
    pub disk_fingerprint: String,
}

pub(crate) fn validate_snapshot_request(
    request: &RecoverySnapshotRequest,
) -> Result<String, LibraryError> {
    if request.document_id.trim().is_empty() {
        return Err(LibraryError::new(
            "recovery_invalid",
            "Recovery document identity is required",
        ));
    }
    if Uuid::parse_str(&request.session_generation).is_err() {
        return Err(LibraryError::new(
            "recovery_invalid",
            "Recovery session generation is invalid",
        ));
    }
    if request.revision <= 0 {
        return Err(LibraryError::new(
            "recovery_invalid",
            "Recovery revision must be positive",
        ));
    }
    if request.bytes.len() > MAX_RECOVERY_BYTES {
        return Err(LibraryError::new(
            "recovery_too_large",
            "Recovery content exceeds the supported size",
        ));
    }
    if std::str::from_utf8(&request.bytes).is_err() {
        return Err(LibraryError::new(
            "recovery_invalid",
            "Editable recovery content must be valid UTF-8",
        ));
    }
    if !is_sha256_fingerprint(&request.base_fingerprint) {
        return Err(LibraryError::new(
            "recovery_invalid",
            "Recovery base fingerprint is invalid",
        ));
    }

    Ok(format!("sha256:{:x}", Sha256::digest(&request.bytes)))
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_snapshot_identity_revision_utf8_and_fingerprint() {
        let request = RecoverySnapshotRequest {
            document_id: "document-1".to_owned(),
            session_generation: Uuid::new_v4().to_string(),
            revision: 1,
            bytes: b"# Draft\n".to_vec(),
            base_fingerprint: format!("sha256:{:064x}", 1),
        };

        assert!(validate_snapshot_request(&request).is_ok());

        let mut invalid = request;
        invalid.bytes = vec![0xff];
        assert_eq!(
            validate_snapshot_request(&invalid).unwrap_err().code(),
            "recovery_invalid"
        );
    }
}
