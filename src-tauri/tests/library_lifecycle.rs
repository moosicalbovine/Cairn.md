use std::fs;

use cairn_md_lib::domain::library::{JournalPhase, LibraryMode, LibraryService};
use tempfile::TempDir;

fn service_and_root() -> (TempDir, TempDir, LibraryService) {
    let app_data = TempDir::new().expect("app-data temp dir");
    let root = TempDir::new().expect("library temp dir");
    let service = LibraryService::open(app_data.path()).expect("open metadata");
    (app_data, root, service)
}

#[test]
fn binds_one_root_and_indexes_only_relative_direct_children() {
    let (_app_data, root, mut service) = service_and_root();
    fs::create_dir(root.path().join("Existing")).unwrap();
    fs::write(
        root.path().join("Existing").join("welcome.md"),
        "# Welcome\n",
    )
    .unwrap();
    fs::create_dir(root.path().join("Existing").join("Nested")).unwrap();
    fs::write(
        root.path()
            .join("Existing")
            .join("Nested")
            .join("hidden.md"),
        "hidden",
    )
    .unwrap();
    fs::write(root.path().join("root.md"), "hidden").unwrap();

    let probe = service.probe_candidate(root.path()).unwrap();
    assert!(probe.can_bind);
    assert!(probe.can_atomic_replace);
    let snapshot = service.bind_root(root.path()).unwrap();

    assert_eq!(snapshot.mode, LibraryMode::Writable);
    assert_eq!(snapshot.binding.as_ref().unwrap().generation, 1);
    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.projects[0].relative_path, "Existing");
    assert_eq!(snapshot.projects[0].documents.len(), 1);
    assert_eq!(
        snapshot.projects[0].documents[0].relative_path,
        "Existing/welcome.md"
    );
    assert!(!snapshot.projects[0]
        .relative_path
        .contains(root.path().to_string_lossy().as_ref()));
}

#[test]
fn project_and_document_lifecycle_preserves_stable_ids() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();

    let alpha = service.create_project("Alpha").unwrap();
    let beta = service.create_project("Beta").unwrap();
    let renamed_alpha = service.rename_project(&alpha.id, "alpha").unwrap();
    assert_eq!(renamed_alpha.id, alpha.id);
    assert_eq!(renamed_alpha.relative_path, "alpha");

    let document = service.create_document(&alpha.id, "Notes.md").unwrap();
    let renamed = service.rename_document(&document.id, "notes.md").unwrap();
    assert_eq!(renamed.id, document.id);
    assert_eq!(renamed.relative_path, "alpha/notes.md");

    let moved = service.move_document(&document.id, &beta.id).unwrap();
    assert_eq!(moved.id, document.id);
    assert_eq!(moved.relative_path, "Beta/notes.md");
    assert!(root.path().join("Beta").join("notes.md").is_file());

    let deleted = service.delete_document(&document.id).unwrap();
    assert!(deleted.recycled);
    assert!(deleted
        .recovery_path
        .as_ref()
        .is_some_and(|path| path.is_file()));
    assert!(!root.path().join("Beta").join("notes.md").exists());
    assert!(service
        .snapshot()
        .unwrap()
        .projects
        .iter()
        .all(|project| project.documents.is_empty()));
    assert_eq!(service.pending_operation_count().unwrap(), 0);
}

#[test]
fn duplicate_names_are_case_folded_and_traversal_is_rejected() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service.create_document(&project.id, "Readme.md").unwrap();

    let duplicate = service
        .create_document(&project.id, "README.md")
        .unwrap_err();
    assert_eq!(duplicate.code(), "path_conflict");
    assert_eq!(
        service.create_project("alpha").unwrap_err().code(),
        "path_conflict"
    );
    assert_eq!(
        service.create_project("../escape").unwrap_err().code(),
        "invalid_path"
    );
    assert_eq!(
        service
            .create_document(&project.id, "../escape.md")
            .unwrap_err()
            .code(),
        "invalid_path"
    );
}

#[test]
fn authoritative_reconcile_is_idempotent_and_keeps_identical_files_separate() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    fs::write(root.path().join("Alpha").join("one.md"), "same").unwrap();
    fs::write(root.path().join("Alpha").join("two.md"), "same").unwrap();
    fs::create_dir(root.path().join("Alpha").join("nested")).unwrap();
    fs::write(
        root.path().join("Alpha").join("nested").join("three.md"),
        "same",
    )
    .unwrap();

    service.reconcile().unwrap();
    let once = service.snapshot().unwrap();
    service.reconcile().unwrap();
    let twice = service.snapshot().unwrap();

    let documents = &twice
        .projects
        .iter()
        .find(|candidate| candidate.id == project.id)
        .unwrap()
        .documents;
    assert_eq!(documents.len(), 2);
    assert_ne!(documents[0].id, documents[1].id);
    assert_eq!(once, twice);
}

#[test]
fn external_rename_rebinds_the_existing_document_without_duplicates() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "before.md").unwrap();
    fs::write(
        root.path().join("Alpha").join("before.md"),
        "changed before rename",
    )
    .unwrap();
    fs::rename(
        root.path().join("Alpha").join("before.md"),
        root.path().join("Alpha").join("after.md"),
    )
    .unwrap();

    service.reconcile().unwrap();
    let snapshot = service.snapshot().unwrap();
    let documents = &snapshot.projects[0].documents;
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document.id);
    assert_eq!(documents[0].relative_path, "Alpha/after.md");
}

#[test]
fn reconciliation_follows_identity_when_two_paths_are_swapped() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let first = service.create_document(&project.id, "first.md").unwrap();
    let second = service.create_document(&project.id, "second.md").unwrap();
    fs::write(root.path().join("Alpha/first.md"), "first").unwrap();
    fs::write(root.path().join("Alpha/second.md"), "second").unwrap();
    service.reconcile().unwrap();

    let temporary = root.path().join("Alpha/swap.tmp");
    fs::rename(root.path().join("Alpha/first.md"), &temporary).unwrap();
    fs::rename(
        root.path().join("Alpha/second.md"),
        root.path().join("Alpha/first.md"),
    )
    .unwrap();
    fs::rename(&temporary, root.path().join("Alpha/second.md")).unwrap();
    service.reconcile().unwrap();

    let documents = service.snapshot().unwrap().projects[0].documents.clone();
    assert_eq!(
        documents
            .iter()
            .find(|document| document.id == first.id)
            .unwrap()
            .relative_path,
        "Alpha/second.md"
    );
    assert_eq!(
        documents
            .iter()
            .find(|document| document.id == second.id)
            .unwrap()
            .relative_path,
        "Alpha/first.md"
    );
}

#[test]
fn reconciliation_never_reuses_one_row_for_two_hard_linked_paths() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service.create_document(&project.id, "one.md").unwrap();
    service.create_document(&project.id, "two.md").unwrap();
    fs::remove_file(root.path().join("Alpha/two.md")).unwrap();
    fs::hard_link(
        root.path().join("Alpha/one.md"),
        root.path().join("Alpha/two.md"),
    )
    .unwrap();

    service.reconcile().unwrap();
    let documents = &service.snapshot().unwrap().projects[0].documents;
    assert_eq!(documents.len(), 2);
    assert_ne!(documents[0].id, documents[1].id);
}

#[test]
fn unique_fingerprint_fallback_preserves_id_when_identity_changes() {
    let (_app_data, root, mut service) = service_and_root();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "before.md").unwrap();
    fs::write(root.path().join("Alpha/before.md"), "unique content").unwrap();
    service.reconcile().unwrap();
    fs::copy(
        root.path().join("Alpha/before.md"),
        root.path().join("Alpha/after.md"),
    )
    .unwrap();
    fs::remove_file(root.path().join("Alpha/before.md")).unwrap();

    service.reconcile().unwrap();
    let documents = &service.snapshot().unwrap().projects[0].documents;
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document.id);
    assert_eq!(documents[0].relative_path, "Alpha/after.md");
}

#[test]
fn relocation_requires_confirmation_and_increments_only_the_binding_generation() {
    let app_data = TempDir::new().unwrap();
    let container = TempDir::new().unwrap();
    let original = container.path().join("original");
    let relocated = container.path().join("relocated");
    fs::create_dir(&original).unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    let first = service.bind_root(&original).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "note.md").unwrap();
    let binding = first.binding.unwrap();
    fs::rename(&original, &relocated).unwrap();

    let preview = service.preview_relink(&relocated).unwrap();
    assert_eq!(preview.matched_projects, 1);
    assert_eq!(preview.matched_documents, 1);
    assert_eq!(
        service.snapshot().unwrap().binding.unwrap().root_path,
        binding.root_path.clone()
    );

    let expected_relocated_path = preview.candidate_path.clone();
    let relinked = service.confirm_relink(preview).unwrap();
    let rebound = relinked.binding.unwrap();
    assert_eq!(rebound.library_id, binding.library_id);
    assert_eq!(rebound.generation, binding.generation + 1);
    assert_eq!(rebound.root_path, expected_relocated_path);
    assert_eq!(relinked.projects[0].id, project.id);
    assert_eq!(relinked.projects[0].documents[0].id, document.id);
}

#[test]
fn confirmed_copy_relocation_preserves_project_and_document_ids() {
    let app_data = TempDir::new().unwrap();
    let original = TempDir::new().unwrap();
    let copied = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(original.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "note.md").unwrap();
    fs::write(original.path().join("Alpha/note.md"), "portable").unwrap();
    service.reconcile().unwrap();

    fs::create_dir(copied.path().join("Alpha")).unwrap();
    fs::copy(
        original.path().join("Alpha/note.md"),
        copied.path().join("Alpha/note.md"),
    )
    .unwrap();

    let preview = service.preview_relink(copied.path()).unwrap();
    let relinked = service.confirm_relink(preview).unwrap();
    assert_eq!(relinked.projects[0].id, project.id);
    assert_eq!(relinked.projects[0].documents[0].id, document.id);
}

#[test]
fn failed_or_stale_relink_preserves_the_previous_binding() {
    let (_app_data, root, mut service) = service_and_root();
    let first = service.bind_root(root.path()).unwrap().binding.unwrap();
    let missing = root.path().join("missing");
    assert_eq!(
        service.preview_relink(&missing).unwrap_err().code(),
        "root_unavailable"
    );
    assert_eq!(service.snapshot().unwrap().binding.unwrap(), first);

    let content_candidate = TempDir::new().unwrap();
    fs::create_dir(content_candidate.path().join("Alpha")).unwrap();
    fs::write(content_candidate.path().join("Alpha/note.md"), "before").unwrap();
    let stale_content = service.preview_relink(content_candidate.path()).unwrap();
    fs::write(content_candidate.path().join("Alpha/note.md"), "after").unwrap();
    assert_eq!(
        service.confirm_relink(stale_content).unwrap_err().code(),
        "stale_relink"
    );
    assert_eq!(service.snapshot().unwrap().binding.unwrap(), first);

    let candidate = TempDir::new().unwrap();
    let stale = service.preview_relink(candidate.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service.rename_project(&project.id, "Beta").unwrap();
    assert_eq!(
        service.confirm_relink(stale).unwrap_err().code(),
        "stale_relink"
    );
    assert_eq!(service.snapshot().unwrap().binding.unwrap(), first);
}

#[test]
fn startup_replays_an_interrupted_operation_once() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service
        .create_document_interrupted_for_test(
            &project.id,
            "replayed.md",
            JournalPhase::FilesystemFinalized,
        )
        .unwrap();
    assert!(root.path().join("Alpha").join("replayed.md").is_file());
    assert!(service.snapshot().unwrap().projects[0].documents.is_empty());
    drop(service);

    let first_restart = LibraryService::open(app_data.path()).unwrap();
    let first_snapshot = first_restart.snapshot().unwrap();
    assert_eq!(first_snapshot.projects[0].documents.len(), 1);
    let document_id = first_snapshot.projects[0].documents[0].id.clone();
    drop(first_restart);

    let second_restart = LibraryService::open(app_data.path()).unwrap();
    let second_snapshot = second_restart.snapshot().unwrap();
    assert_eq!(second_snapshot.projects[0].documents.len(), 1);
    assert_eq!(second_snapshot.projects[0].documents[0].id, document_id);
    assert_eq!(second_restart.pending_operation_count().unwrap(), 0);
}

#[test]
fn startup_replays_create_after_final_rename_before_phase_update() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service
        .create_document_interrupted_after_finalize_before_phase_for_test(&project.id, "window.md")
        .unwrap();
    assert!(root.path().join("Alpha/window.md").is_file());
    drop(service);

    let restarted = LibraryService::open(app_data.path()).unwrap();
    let snapshot = restarted.snapshot().unwrap();
    assert_eq!(snapshot.mode, LibraryMode::Writable);
    assert_eq!(snapshot.projects[0].documents.len(), 1);
    assert_eq!(
        snapshot.projects[0].documents[0].relative_path,
        "Alpha/window.md"
    );
    assert_eq!(restarted.pending_operation_count().unwrap(), 0);
}

#[test]
fn replay_mismatch_opens_read_only_and_preserves_pending_evidence() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service
        .create_document_interrupted_after_finalize_before_phase_for_test(
            &project.id,
            "mismatch.md",
        )
        .unwrap();
    fs::write(root.path().join("Alpha/mismatch.md"), "changed").unwrap();
    drop(service);

    let restarted = LibraryService::open(app_data.path()).unwrap();
    let snapshot = restarted.snapshot().unwrap();
    assert_eq!(snapshot.mode, LibraryMode::ReadOnly);
    assert_eq!(
        snapshot.read_only_reason.as_deref(),
        Some("journal_recovery_failed")
    );
    assert_eq!(restarted.pending_operation_count().unwrap(), 1);
}

#[test]
fn startup_rejects_an_unrelated_directory_at_the_bound_path_before_mutating_it() {
    let app_data = TempDir::new().unwrap();
    let container = TempDir::new().unwrap();
    let bound = container.path().join("bound");
    let relocated = container.path().join("relocated");
    fs::create_dir(&bound).unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(&bound).unwrap();
    service.create_project("Original").unwrap();
    drop(service);

    fs::rename(&bound, &relocated).unwrap();
    fs::create_dir(&bound).unwrap();
    fs::write(bound.join("unrelated.txt"), "leave me alone").unwrap();

    let restarted = LibraryService::open(app_data.path()).unwrap();
    let snapshot = restarted.snapshot().unwrap();
    assert_eq!(snapshot.mode, LibraryMode::ReadOnly);
    assert_eq!(
        snapshot.read_only_reason.as_deref(),
        Some("root_identity_mismatch")
    );
    let entries = fs::read_dir(&bound)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec!["unrelated.txt".to_owned()]);
}

#[test]
fn cleanup_complete_replay_never_repeats_a_delete() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    let bound = service.bind_root(root.path()).unwrap();
    let library_id = bound.binding.unwrap().library_id;
    let project = service.create_project("Alpha").unwrap();
    let document = service.create_document(&project.id, "note.md").unwrap();
    fs::write(root.path().join("Alpha/note.md"), "restored content").unwrap();
    service.reconcile().unwrap();
    service.delete_document(&document.id).unwrap();
    fs::write(root.path().join("Alpha/note.md"), "restored content").unwrap();
    drop(service);

    let database_path = app_data.path().join("library.sqlite3");
    let connection = rusqlite::Connection::open(database_path).unwrap();
    let payload = serde_json::json!({
        "kind": "delete_document",
        "document_id": document.id,
        "relative_path": "Alpha/note.md",
        "recovery_relative_path": "Alpha/.cairn-recovery-finished.md"
    });
    connection
        .execute(
            "INSERT INTO pending_file_operations (id, library_id, kind, phase, payload_json, expected_fingerprint, finalized_fingerprint, temporary_path, created_at, updated_at) VALUES ('finished-delete', ?1, 'delete_document', 'Cleanup complete', ?2, NULL, NULL, NULL, 1, 1)",
            rusqlite::params![library_id, payload.to_string()],
        )
        .unwrap();
    drop(connection);

    let restarted = LibraryService::open(app_data.path()).unwrap();
    assert_eq!(restarted.pending_operation_count().unwrap(), 0);
    assert_eq!(
        fs::read_to_string(root.path().join("Alpha/note.md")).unwrap(),
        "restored content"
    );
}

#[test]
fn damaged_metadata_fails_closed_without_replacement() {
    let app_data = TempDir::new().unwrap();
    fs::create_dir_all(app_data.path()).unwrap();
    let database_path = app_data.path().join("library.sqlite3");
    let damaged = b"not a sqlite database";
    fs::write(&database_path, damaged).unwrap();

    let mut service = LibraryService::open(app_data.path()).unwrap();
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.mode, LibraryMode::ReadOnly);
    assert_eq!(
        snapshot.read_only_reason.as_deref(),
        Some("metadata_damaged")
    );
    assert_eq!(
        service.create_project("Nope").unwrap_err().code(),
        "metadata_damaged"
    );
    assert_eq!(fs::read(database_path).unwrap(), damaged);
    assert!(fs::read_dir(app_data.path().join("metadata-quarantine"))
        .unwrap()
        .next()
        .is_some());
}

#[test]
fn incompatible_interrupted_migration_fails_closed_with_evidence() {
    let app_data = TempDir::new().unwrap();
    let database_path = app_data.path().join("library.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch("CREATE VIEW libraries AS SELECT 1 AS id; PRAGMA user_version = 0;")
        .unwrap();
    drop(connection);

    let service = LibraryService::open(app_data.path()).unwrap();
    let snapshot = service.snapshot().unwrap();
    assert_eq!(snapshot.mode, LibraryMode::ReadOnly);
    assert_eq!(
        snapshot.read_only_reason.as_deref(),
        Some("metadata_damaged")
    );
    let preserved = rusqlite::Connection::open(&database_path).unwrap();
    let object_type: String = preserved
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = 'libraries'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(object_type, "view");
    assert!(fs::read_dir(app_data.path().join("metadata-quarantine"))
        .unwrap()
        .next()
        .is_some());
}

#[test]
fn unavailable_candidate_and_non_directory_candidate_fail_capability_probe() {
    let (_app_data, root, service) = service_and_root();
    let missing = root.path().join("missing");
    let file = root.path().join("file");
    fs::write(&file, "x").unwrap();

    assert!(!service.probe_candidate(&missing).unwrap().can_bind);
    assert!(!service.probe_candidate(&file).unwrap().can_bind);
}

#[cfg(windows)]
#[test]
fn reparse_point_projects_are_never_indexed() {
    use std::process::Command;

    let (_app_data, root, mut service) = service_and_root();
    let outside = TempDir::new().unwrap();
    let link = root.path().join("Escape");
    let status = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(&link)
        .arg(outside.path())
        .status()
        .expect("run mklink junction fixture");
    assert!(status.success(), "junction fixture must be exercised");
    service.bind_root(root.path()).unwrap();
    assert!(service.snapshot().unwrap().projects.is_empty());
}
