use std::collections::HashMap;
use std::fs;

use cairn_md_lib::domain::library::LibraryService;
use tempfile::TempDir;

const PROJECT_COUNT: usize = 20;
const DOCUMENTS_PER_PROJECT: usize = 50;

fn document_source(project: usize, document: usize, generation: usize) -> String {
    format!("# Project {project} document {document}\n\nGeneration {generation}.\n")
}

#[test]
fn repeated_large_library_reconciliation_preserves_ids_and_content() {
    let app_data = TempDir::new().unwrap();
    let root = TempDir::new().unwrap();
    for project in 0..PROJECT_COUNT {
        let project_path = root.path().join(format!("Project-{project:02}"));
        fs::create_dir(&project_path).unwrap();
        for document in 0..DOCUMENTS_PER_PROJECT {
            fs::write(
                project_path.join(format!("document-{document:03}.md")),
                document_source(project, document, 0),
            )
            .unwrap();
        }
    }

    let mut service = LibraryService::open(app_data.path()).unwrap();
    let initial = service.bind_root(root.path()).unwrap();
    let initial_ids = initial
        .projects
        .iter()
        .flat_map(|project| &project.documents)
        .map(|document| (document.relative_path.clone(), document.id.clone()))
        .collect::<HashMap<_, _>>();
    assert_eq!(initial_ids.len(), PROJECT_COUNT * DOCUMENTS_PER_PROJECT);

    for project in 0..PROJECT_COUNT {
        let project_path = root.path().join(format!("Project-{project:02}"));
        fs::write(
            project_path.join("document-001.md"),
            document_source(project, 1, 1),
        )
        .unwrap();
        fs::rename(
            project_path.join("document-000.md"),
            project_path.join("renamed-000.md"),
        )
        .unwrap();
    }

    for _ in 0..5 {
        service.reconcile().unwrap();
    }
    let reconciled = service.snapshot().unwrap();
    let documents = reconciled
        .projects
        .iter()
        .flat_map(|project| &project.documents)
        .collect::<Vec<_>>();
    assert_eq!(documents.len(), PROJECT_COUNT * DOCUMENTS_PER_PROJECT);

    for project in 0..PROJECT_COUNT {
        let old_path = format!("Project-{project:02}/document-000.md");
        let renamed_path = format!("Project-{project:02}/renamed-000.md");
        let renamed = documents
            .iter()
            .find(|document| document.relative_path == renamed_path)
            .unwrap();
        assert_eq!(Some(&renamed.id), initial_ids.get(&old_path));
        assert_eq!(
            fs::read_to_string(
                root.path()
                    .join(format!("Project-{project:02}/document-001.md"))
            )
            .unwrap(),
            document_source(project, 1, 1)
        );
    }

    drop(service);
    let restarted = LibraryService::open(app_data.path()).unwrap();
    let after_restart = restarted.snapshot().unwrap();
    assert_eq!(
        after_restart
            .projects
            .iter()
            .map(|project| project.documents.len())
            .sum::<usize>(),
        PROJECT_COUNT * DOCUMENTS_PER_PROJECT
    );
}
