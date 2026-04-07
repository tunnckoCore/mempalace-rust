mod common;

use std::fs;

use mempalace_rust::{convo, project};

#[test]
fn project_mining_and_search_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project_dir = dir.path().join("app");
    fs::create_dir_all(project_dir.join("backend")).expect("mkdir backend");
    fs::write(
        project_dir.join("backend/app.py"),
        "def main():\n    print('hello world')\n# graphql migration decision\n".repeat(20),
    )
    .expect("write app.py");
    fs::write(
        project_dir.join("mempalace.yaml"),
        "wing: test_project\nrooms:\n  - name: backend\n  - name: general\n",
    )
    .expect("write config");

    let palace = dir.path().join("palace");
    let mut storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    let summary = project::mine_project(&project_dir, &mut storage, None, "tester", None, false)
        .expect("mine project");
    assert!(summary.drawers_added > 0);

    let hits = storage
        .search("graphql migration", None, None, 5)
        .expect("search");
    assert!(!hits.is_empty());
}

#[test]
fn convo_mining_normalizes_and_indexes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let convos_dir = dir.path().join("convos");
    fs::create_dir_all(&convos_dir).expect("mkdir convos");
    fs::write(
        convos_dir.join("chat.txt"),
        "> What is memory?\nMemory is persistence.\n\n> Why does it matter?\nIt enables continuity.\n\n> How do we build it?\nWith structured storage.\n",
    )
    .expect("write convo");

    let palace = dir.path().join("palace");
    let mut storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    let summary = convo::mine_conversations(
        &convos_dir,
        &mut storage,
        Some("test_convos"),
        "tester",
        None,
        false,
        "exchange",
    )
    .expect("mine convos");
    assert!(summary.drawers_added >= 2);
}
