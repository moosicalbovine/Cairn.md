use cairn_md_lib::domain::library::{JournalPhase, LibraryService};
use std::fs;

use cairn_md_lib::domain::recovery::{RecoveryLifecycle, RecoverySnapshotRequest, SaveStatus};
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

#[test]
fn save_requires_durable_recovery_then_atomically_advances_disk_and_metadata() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let draft = request(&document_id, &generation, 1, &base_fingerprint, "# Saved\n");
    service.store_recovery_snapshot(draft.clone()).unwrap();

    let saved = service.save_document(draft).unwrap();

    assert_eq!(saved.status, SaveStatus::Saved);
    assert_eq!(saved.revision, 1);
    assert_eq!(
        fs::read(root.path().join("Alpha/draft.md")).unwrap(),
        b"# Saved\n"
    );
    assert_eq!(
        service
            .read_document(&document_id)
            .unwrap()
            .base_fingerprint,
        saved.disk_fingerprint
    );
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_none());
    assert_eq!(service.pending_operation_count().unwrap(), 0);
}

#[test]
fn external_change_is_preserved_beside_the_recoverable_draft() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let draft = request(
        &document_id,
        &generation,
        1,
        &base_fingerprint,
        "local draft",
    );
    service.store_recovery_snapshot(draft.clone()).unwrap();
    fs::write(root.path().join("Alpha/draft.md"), "external edit").unwrap();

    let conflict = service.save_document(draft).unwrap();

    assert_eq!(conflict.status, SaveStatus::Conflict);
    assert_eq!(
        fs::read_to_string(root.path().join("Alpha/draft.md")).unwrap(),
        "external edit"
    );
    let recovery = service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .expect("draft remains recoverable");
    assert_eq!(recovery.bytes, b"local draft");
    assert_eq!(recovery.lifecycle_state, RecoveryLifecycle::Conflict);
    assert_eq!(service.pending_operation_count().unwrap(), 0);

    fs::write(root.path().join("Alpha/draft.md"), "external edit again").unwrap();
    let recovered_copy = service
        .save_recovery_copy(&document_id, &generation)
        .unwrap();
    assert_eq!(recovered_copy.relative_path, "Alpha/draft (recovered).md");
    assert_eq!(
        fs::read(root.path().join(&recovered_copy.relative_path)).unwrap(),
        b"local draft"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("Alpha/draft.md")).unwrap(),
        "external edit again"
    );
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_none());
}

#[test]
fn deleting_external_file_during_resolution_does_not_lose_the_local_draft() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let draft = request(
        &document_id,
        &generation,
        1,
        &base_fingerprint,
        "local survives deletion",
    );
    service.store_recovery_snapshot(draft.clone()).unwrap();
    fs::write(root.path().join("Alpha/draft.md"), "external").unwrap();
    assert_eq!(
        service.save_document(draft).unwrap().status,
        SaveStatus::Conflict
    );
    fs::remove_file(root.path().join("Alpha/draft.md")).unwrap();

    let recovered_copy = service
        .save_recovery_copy(&document_id, &generation)
        .unwrap();

    assert_eq!(
        fs::read(root.path().join(recovered_copy.relative_path)).unwrap(),
        b"local survives deletion"
    );
    assert!(!root.path().join("Alpha/draft.md").exists());
}

#[test]
fn reconciliation_preserves_recovery_when_the_external_project_disappears() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    service
        .store_recovery_snapshot(request(
            &document_id,
            &generation,
            1,
            &base_fingerprint,
            "local survives reconciliation",
        ))
        .unwrap();
    fs::remove_dir_all(root.path().join("Alpha")).unwrap();

    let snapshot = service.reconcile().unwrap();

    let retained = snapshot
        .projects
        .iter()
        .flat_map(|project| &project.documents)
        .find(|document| document.id == document_id)
        .expect("document metadata remains reachable while recovery is pending");
    assert_eq!(retained.relative_path, "Alpha/draft.md");
    let recovery = service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .expect("recovery survives watcher reconciliation");
    assert_eq!(recovery.bytes, b"local survives reconciliation");

    let recovered_copy = service
        .save_recovery_copy(&document_id, &generation)
        .unwrap();
    assert_eq!(
        fs::read(root.path().join(recovered_copy.relative_path)).unwrap(),
        b"local survives reconciliation"
    );
}

#[test]
fn reconciliation_rebinds_a_recreated_project_without_orphaning_recovery() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    service
        .store_recovery_snapshot(request(
            &document_id,
            &generation,
            1,
            &base_fingerprint,
            "local survives replacement",
        ))
        .unwrap();
    fs::remove_dir_all(root.path().join("Alpha")).unwrap();
    fs::create_dir(root.path().join("Alpha")).unwrap();
    fs::write(root.path().join("Alpha/external.md"), "external").unwrap();

    let snapshot = service.reconcile().unwrap();

    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.projects[0].relative_path, "Alpha");
    assert!(snapshot.projects[0]
        .documents
        .iter()
        .any(|document| document.id == document_id));
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_some());
}

#[test]
fn explicit_reload_accepts_the_latest_external_bytes_and_clears_recovery() {
    let (_app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let draft = request(
        &document_id,
        &generation,
        1,
        &base_fingerprint,
        "local draft",
    );
    service.store_recovery_snapshot(draft.clone()).unwrap();
    fs::write(root.path().join("Alpha/draft.md"), "external one").unwrap();
    assert_eq!(
        service.save_document(draft).unwrap().status,
        SaveStatus::Conflict
    );
    fs::write(root.path().join("Alpha/draft.md"), "external two").unwrap();

    let reloaded = service
        .reload_document_from_disk(&document_id, Some(&generation))
        .unwrap();

    assert_eq!(reloaded.bytes, b"external two");
    assert_eq!(
        reloaded.base_fingerprint,
        reloaded.document.disk_fingerprint
    );
    assert!(service
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_none());
}

#[test]
fn save_journal_replays_every_recorded_phase_and_is_idempotent() {
    for phase in [
        JournalPhase::IntentRecorded,
        JournalPhase::TemporaryDurable,
        JournalPhase::FilesystemFinalized,
        JournalPhase::MetadataCommitted,
        JournalPhase::CleanupComplete,
    ] {
        let (app_data, root, mut service, document_id, base_fingerprint) = bound_document();
        let generation = Uuid::new_v4().to_string();
        let draft = request(
            &document_id,
            &generation,
            1,
            &base_fingerprint,
            "journal replay",
        );
        service.store_recovery_snapshot(draft.clone()).unwrap();
        service
            .save_document_interrupted_for_test(draft, phase)
            .unwrap();
        drop(service);

        let reopened = LibraryService::open(app_data.path()).unwrap();
        assert_eq!(
            fs::read(root.path().join("Alpha/draft.md")).unwrap(),
            b"journal replay",
            "phase {phase:?}"
        );
        assert!(reopened
            .load_recovery_snapshot(&document_id)
            .unwrap()
            .is_none());
        assert_eq!(reopened.pending_operation_count().unwrap(), 0);
        assert!(fs::read_dir(root.path().join("Alpha"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".cairn-save-")));
        drop(reopened);

        let reopened_again = LibraryService::open(app_data.path()).unwrap();
        assert_eq!(reopened_again.pending_operation_count().unwrap(), 0);
        assert_eq!(
            fs::read(root.path().join("Alpha/draft.md")).unwrap(),
            b"journal replay"
        );
    }
}

#[test]
fn save_journal_recognizes_replacement_completed_before_phase_record() {
    let (app_data, root, mut service, document_id, base_fingerprint) = bound_document();
    let generation = Uuid::new_v4().to_string();
    let draft = request(
        &document_id,
        &generation,
        1,
        &base_fingerprint,
        "replace completed",
    );
    service.store_recovery_snapshot(draft.clone()).unwrap();
    service
        .save_document_interrupted_after_replace_before_phase_for_test(draft)
        .unwrap();
    drop(service);

    let reopened = LibraryService::open(app_data.path()).unwrap();
    assert_eq!(
        fs::read(root.path().join("Alpha/draft.md")).unwrap(),
        b"replace completed"
    );
    assert_eq!(reopened.pending_operation_count().unwrap(), 0);
    assert!(reopened
        .load_recovery_snapshot(&document_id)
        .unwrap()
        .is_none());
}
