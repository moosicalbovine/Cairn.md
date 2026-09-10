use std::fs;
use std::path::PathBuf;

use cairn_md_lib::domain::import::ImportSource;
use cairn_md_lib::domain::library::LibraryService;
use tempfile::TempDir;

fn external(path: PathBuf) -> ImportSource {
    ImportSource::ExternalPath {
        absolute_path: path,
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
