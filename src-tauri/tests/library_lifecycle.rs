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
    assert!(deleted.recovery_path.is_file());
    assert!(!root.path().join("Beta").join("notes.md").exists());
    assert!(service
        .snapshot()
        .unwrap()
        .projects
        .iter()
        .all(|project| project.documents.is_empty()));
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
        original
    );

    let relinked = service.confirm_relink(preview).unwrap();
    let rebound = relinked.binding.unwrap();
    assert_eq!(rebound.library_id, binding.library_id);
    assert_eq!(rebound.generation, binding.generation + 1);
    assert_eq!(rebound.root_path, relocated);
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
    use std::os::windows::fs::symlink_dir;

    let (_app_data, root, mut service) = service_and_root();
    let outside = TempDir::new().unwrap();
    if symlink_dir(outside.path(), root.path().join("Escape")).is_err() {
        return;
    }
    service.bind_root(root.path()).unwrap();
    assert!(service.snapshot().unwrap().projects.is_empty());
}
