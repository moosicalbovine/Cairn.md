use cairn_md_lib::domain::library::LibraryService;
use cairn_md_lib::domain::recovery::{RecoveryLifecycle, RecoverySnapshotRequest};
use tempfile::TempDir;
use uuid::Uuid;

fn bound_document() -> (TempDir, TempDir, LibraryService, String, String) {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "draft.md").unwrap();
    let base_fingerprint = document.disk_fingerprint.clone();
    (app_data, root, service, document.id, base_fingerprint)
}

fn request(
    document_id: &str,
    generation: &str,
    revision: i64,
    base_fingerprint: &str,
    source: &str,
) -> RecoverySnapshotRequest {
    RecoverySnapshotRequest {
        document_id: document_id.to_owned(),
        session_generation: generation.to_owned(),
        revision,
        bytes: source.as_bytes().to_vec(),
        base_fingerprint: base_fingerprint.to_owned(),
    }
}

#[test]
fn newest_snapshot_is_durable_across_restart_and_stale_revisions_cannot_replace_it() {
    let (app_data, _root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();

    service
        .store_recovery_snapshot(request(
            &document_id,
            &generation,
            2,
            &base_fingerprint,
            "newest",
        ))
        .unwrap();
    let stale_result = service
        .store_recovery_snapshot(request(
            &document_id,
            &generation,
            1,
            &base_fingerprint,
            "stale",
        ))
        .unwrap();
    assert_eq!(stale_result.revision, 2);
    assert_eq!(stale_result.bytes, b"newest");

    drop(service);
    let reopened = LibraryService::open(app_data.path()).unwrap();
    let restored = reopened
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .expect("durable recovery snapshot");
    assert_eq!(restored.revision, 2);
    assert_eq!(restored.bytes, b"newest");
    assert_eq!(restored.lifecycle_state, RecoveryLifecycle::Draft);
    assert_eq!(restored.content_hash, restored.intended_disk_hash);
}

#[test]
fn another_session_cannot_overwrite_or_discard_pending_recovery() {
    let (_app_data, _root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let other_generation = Uuid::new_v4().to_string();
    service
        .store_recovery_snapshot(request(
            &document_id,
            &generation,
            1,
            &base_fingerprint,
            "recover me",
        ))
        .unwrap();

    let error = service
        .store_recovery_snapshot(request(
            &document_id,
            &other_generation,
            2,
            &base_fingerprint,
            "overwrite",
        ))
        .unwrap_err();
    assert_eq!(error.code(), "recovery_pending");
    assert!(!service
        .discard_recovery_snapshot(&document_id, &other_generation)
        .unwrap());
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_some());

    assert!(service
        .discard_recovery_snapshot(&document_id, &generation)
        .unwrap());
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_none());
}
