use std::fs;
use std::path::PathBuf;

use cairn_md_lib::domain::import::ImportSource;
use cairn_md_lib::domain::library::{JournalPhase, LibraryService};
use cairn_md_lib::infrastructure::copy::{copy_stable_source, inspect_markdown_source};
use tempfile::TempDir;

fn external(path: PathBuf) -> ImportSource {
    ImportSource::ExternalPath {
        absolute_path: path,
    }
}

fn tracked(tracked_folder_id: &str, relative_path: &str) -> ImportSource {
    ImportSource::TrackedFile {
        tracked_folder_id: tracked_folder_id.to_owned(),
        relative_path: relative_path.to_owned(),
    }
}

fn service_and_root() -> (TempDir, TempDir, LibraryService) {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    (app_data, root, service)
}

#[test]
fn imports_case_insensitive_markdown_with_exact_provenance() {
    let (_app_data, root, mut service) = service_and_root();
    let sources = TempDir::new().unwrap();
    let source = sources.path().join("Proposal.MD");
    fs::write(&source, "# Proposal\r\n").unwrap();
    let source_modified = fs::metadata(&source).unwrap().modified().unwrap();
    let project = service.create_project("Alpha").unwrap();

    let imported = service
        .import_document(&project.id, external(source.clone()))
        .unwrap();

    assert_eq!(imported.relative_path, "Alpha/Proposal.MD");
    assert_eq!(
        imported.source_path.as_deref(),
        source.to_str(),
        "the selected source path is app-owned provenance"
    );
    assert!(imported.imported_at.is_some());
    assert_eq!(
        fs::read(root.path().join("Alpha/Proposal.MD")).unwrap(),
        b"# Proposal\r\n"
    );
    assert_eq!(
        fs::metadata(&source).unwrap().modified().unwrap(),
        source_modified,
        "import must not touch the source timestamp"
    );
}

#[test]
fn repeated_imports_are_independent_and_fill_the_smallest_collision_gap() {
    let (_app_data, root, mut service) = service_and_root();
    let sources = TempDir::new().unwrap();
    let source = sources.path().join("proposal.md");
    fs::write(&source, "portable").unwrap();
    let alpha = service.create_project("Alpha").unwrap();
    let beta = service.create_project("Beta").unwrap();
    service.create_document(&alpha.id, "proposal.md").unwrap();
    service
        .create_document(&alpha.id, "proposal (3).md")
        .unwrap();

    let first = service
        .import_document(&alpha.id, external(source.clone()))
        .unwrap();
    let second = service
        .import_document(&alpha.id, external(source.clone()))
        .unwrap();
    let other_project = service
        .import_document(&beta.id, external(source.clone()))
        .unwrap();

    assert_eq!(first.relative_path, "Alpha/proposal (2).md");
    assert_eq!(second.relative_path, "Alpha/proposal (4).md");
    assert_eq!(other_project.relative_path, "Beta/proposal.md");
    assert_ne!(first.id, second.id);
    assert_ne!(first.id, other_project.id);
    fs::write(root.path().join(&first.relative_path), "changed copy").unwrap();
    assert_eq!(fs::read_to_string(&source).unwrap(), "portable");
    assert_eq!(
        fs::read_to_string(root.path().join(&second.relative_path)).unwrap(),
        "portable"
    );
}

#[test]
fn an_existing_library_document_can_be_imported_as_a_new_copy() {
    let (_app_data, root, mut service) = service_and_root();
    let alpha = service.create_project("Alpha").unwrap();
    let beta = service.create_project("Beta").unwrap();
    let original = service.create_document(&alpha.id, "note.md").unwrap();
    let original_path = root.path().join(&original.relative_path);
    fs::write(&original_path, "from library").unwrap();
    service.reconcile().unwrap();

    let imported = service
        .import_document(&beta.id, external(original_path.clone()))
        .unwrap();

    assert_ne!(imported.id, original.id);
    assert_eq!(imported.relative_path, "Beta/note.md");
    assert_eq!(imported.source_path.as_deref(), original_path.to_str());
    assert_eq!(
        fs::read_to_string(root.path().join(&imported.relative_path)).unwrap(),
        "from library"
    );
}

#[test]
fn rejects_non_markdown_and_relative_sources_without_creating_metadata() {
    let (_app_data, _root, mut service) = service_and_root();
    let sources = TempDir::new().unwrap();
    let source = sources.path().join("notes.txt");
    fs::write(&source, "not Markdown").unwrap();
    let project = service.create_project("Alpha").unwrap();

    let error = service
        .import_document(&project.id, external(source))
        .unwrap_err();
    assert_eq!(error.code(), "source_not_markdown");
    let relative_error = service
        .import_document(&project.id, external(PathBuf::from("notes.md")))
        .unwrap_err();
    assert_eq!(relative_error.code(), "source_unavailable");
    assert!(service.snapshot().unwrap().projects[0].documents.is_empty());
    assert_eq!(service.pending_operation_count().unwrap(), 0);
}

#[test]
fn tracked_folders_are_browsed_lazily_and_import_through_the_same_copy_pipeline() {
    let (_app_data, root, mut service) = service_and_root();
    let tracked_root = TempDir::new().unwrap();
    fs::create_dir(tracked_root.path().join("Planning")).unwrap();
    fs::write(tracked_root.path().join("overview.md"), "# Overview").unwrap();
    fs::write(
        tracked_root.path().join("Planning").join("Proposal.MD"),
        "# Proposal",
    )
    .unwrap();
    fs::write(tracked_root.path().join("ignore.txt"), "not Markdown").unwrap();
    let project = service.create_project("Alpha").unwrap();

    let folder = service.add_tracked_folder(tracked_root.path()).unwrap();
    let folders = service.list_tracked_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, folder.id);
    assert!(folders[0].available);
    assert!(folders[0].last_scan_at.is_none());

    let root_entries = service
        .list_tracked_folder_entries(&folder.id, None)
        .unwrap();
    assert_eq!(root_entries.len(), 2);
    assert!(root_entries[0].is_directory);
    assert_eq!(root_entries[0].relative_path, "Planning");
    assert_eq!(root_entries[1].relative_path, "overview.md");
    assert!(service.list_tracked_folders().unwrap()[0]
        .last_scan_at
        .is_some());

    let nested_entries = service
        .list_tracked_folder_entries(&folder.id, Some("Planning"))
        .unwrap();
    assert_eq!(nested_entries.len(), 1);
    assert_eq!(nested_entries[0].relative_path, "Planning/Proposal.MD");

    let imported = service
        .import_document(&project.id, tracked(&folder.id, "Planning/Proposal.MD"))
        .unwrap();
    assert_eq!(imported.relative_path, "Alpha/Proposal.MD");
    assert_eq!(
        imported.source_path.as_deref(),
        folder
            .absolute_path
            .join("Planning")
            .join("Proposal.MD")
            .to_str()
    );
    assert_eq!(
        fs::read_to_string(root.path().join(imported.relative_path)).unwrap(),
        "# Proposal"
    );
}

#[test]
fn tracked_folder_overlap_and_path_traversal_are_rejected() {
    let (_app_data, root, mut service) = service_and_root();
    let project = service.create_project("Alpha").unwrap();

    let same_root = service.add_tracked_folder(root.path()).unwrap_err();
    assert_eq!(same_root.code(), "tracked_folder_overlap");
    let descendant = service
        .add_tracked_folder(&root.path().join("Alpha"))
        .unwrap_err();
    assert_eq!(descendant.code(), "tracked_folder_overlap");
    let ancestor = service
        .add_tracked_folder(root.path().parent().unwrap())
        .unwrap_err();
    assert_eq!(ancestor.code(), "tracked_folder_overlap");

    let tracked_root = TempDir::new().unwrap();
    fs::write(tracked_root.path().join("note.md"), "safe").unwrap();
    let folder = service.add_tracked_folder(tracked_root.path()).unwrap();
    let traversal = service
        .import_document(&project.id, tracked(&folder.id, "../note.md"))
        .unwrap_err();
    assert_eq!(traversal.code(), "invalid_path");
    assert!(service.snapshot().unwrap().projects[0].documents.is_empty());
}

#[test]
fn removing_a_tracked_folder_only_removes_app_metadata() {
    let (_app_data, _root, mut service) = service_and_root();
    let tracked_root = TempDir::new().unwrap();
    let source = tracked_root.path().join("keep.md");
    fs::write(&source, "keep me").unwrap();
    let folder = service.add_tracked_folder(tracked_root.path()).unwrap();

    service.remove_tracked_folder(&folder.id).unwrap();

    assert!(service.list_tracked_folders().unwrap().is_empty());
    assert_eq!(fs::read_to_string(source).unwrap(), "keep me");
    let missing = service.remove_tracked_folder(&folder.id).unwrap_err();
    assert_eq!(missing.code(), "tracked_folder_not_found");
}

#[test]
fn unavailable_tracked_folders_are_reported_without_losing_the_record() {
    let (_app_data, _root, mut service) = service_and_root();
    let parent = TempDir::new().unwrap();
    let tracked_path = parent.path().join("tracked");
    fs::create_dir(&tracked_path).unwrap();
    fs::write(tracked_path.join("note.md"), "before").unwrap();
    let folder = service.add_tracked_folder(&tracked_path).unwrap();
    fs::remove_dir_all(&tracked_path).unwrap();

    let folders = service.list_tracked_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert!(!folders[0].available);
    let error = service
        .list_tracked_folder_entries(&folder.id, None)
        .unwrap_err();
    assert_eq!(error.code(), "tracked_folder_unavailable");
}

#[test]
fn changed_or_disappearing_sources_leave_no_partial_copy() {
    let sources = TempDir::new().unwrap();
    let source = sources.path().join("note.md");
    let changed_target = sources.path().join("changed-copy.tmp");
    fs::write(&source, "before").unwrap();
    let changed_descriptor = inspect_markdown_source(&source).unwrap();
    fs::write(&source, "after").unwrap();

    let changed = copy_stable_source(&changed_descriptor, &changed_target).unwrap_err();
    assert_eq!(changed.code(), "source_changed");
    assert!(!changed_target.exists());

    let missing_target = sources.path().join("missing-copy.tmp");
    let missing_descriptor = inspect_markdown_source(&source).unwrap();
    fs::remove_file(&source).unwrap();
    let missing = copy_stable_source(&missing_descriptor, &missing_target).unwrap_err();
    assert_eq!(missing.code(), "source_unreadable");
    assert!(!missing_target.exists());
}

#[test]
fn interrupted_imports_replay_once_at_every_recorded_phase() {
    for phase in [
        JournalPhase::IntentRecorded,
        JournalPhase::TemporaryDurable,
        JournalPhase::FilesystemFinalized,
        JournalPhase::MetadataCommitted,
    ] {
        let app_data = TempDir::new().unwrap();
        let root = TempDir::new().unwrap();
        let sources = TempDir::new().unwrap();
        let source = sources.path().join("note.md");
        fs::write(&source, "durable import").unwrap();
        let mut service = LibraryService::open(app_data.path()).unwrap();
        service.bind_root(root.path()).unwrap();
        let project = service.create_project("Alpha").unwrap();
        service
            .import_document_interrupted_for_test(&project.id, external(source.clone()), phase)
            .unwrap();
        assert_eq!(service.pending_operation_count().unwrap(), 1);
        drop(service);

        let repaired = LibraryService::open(app_data.path()).unwrap();
        let snapshot = repaired.snapshot().unwrap();
        assert_eq!(snapshot.projects[0].documents.len(), 1, "phase {phase:?}");
        assert_eq!(
            fs::read_to_string(root.path().join("Alpha/note.md")).unwrap(),
            "durable import"
        );
        assert_eq!(repaired.pending_operation_count().unwrap(), 0);
        drop(repaired);

        let second_restart = LibraryService::open(app_data.path()).unwrap();
        assert_eq!(
            second_restart.snapshot().unwrap().projects[0]
                .documents
                .len(),
            1
        );
        assert_eq!(second_restart.pending_operation_count().unwrap(), 0);
        assert_eq!(fs::read_to_string(&source).unwrap(), "durable import");
    }
}

#[test]
fn import_replays_when_finalized_before_the_phase_update() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    let sources = TempDir::new().unwrap();
    let source = sources.path().join("note.md");
    fs::write(&source, "finalized import").unwrap();
    let mut service = LibraryService::open(app_data.path()).unwrap();
    service.bind_root(root.path()).unwrap();
    let project = service.create_project("Alpha").unwrap();
    service
        .import_document_interrupted_after_finalize_before_phase_for_test(
            &project.id,
            external(source),
        )
        .unwrap();
    assert!(root.path().join("Alpha/note.md").is_file());
    assert!(service.snapshot().unwrap().projects[0].documents.is_empty());
    drop(service);

    let repaired = LibraryService::open(app_data.path()).unwrap();
    assert_eq!(repaired.snapshot().unwrap().projects[0].documents.len(), 1);
    assert_eq!(repaired.pending_operation_count().unwrap(), 0);
}
