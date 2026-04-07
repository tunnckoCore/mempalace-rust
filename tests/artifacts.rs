mod common;

use std::fs;

use mempalace_rust::{artifacts, project};

#[test]
fn artifact_aware_retrieval_boosts_raw_parent_hit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project_dir = dir.path().join("artifact-app");
    fs::create_dir_all(project_dir.join("docs")).expect("mkdir docs");
    fs::write(
        project_dir.join("docs/decisions.md"),
        "Kai prefers Clerk. The team decided to switch to Clerk because onboarding and DX were better.",
    )
    .expect("write decisions");
    fs::write(
        project_dir.join("mempalace.yaml"),
        "wing: artifact_app\nrooms:\n  - name: docs\n  - name: general\n",
    )
    .expect("write config");

    let palace = dir.path().join("palace");
    let mut storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    let summary = project::mine_project(&project_dir, &mut storage, None, "tester", None, false)
        .expect("mine project");
    assert!(summary.drawers_added > 0);

    let hits = storage
        .search("Kai prefers Clerk", None, None, 5)
        .expect("search");
    assert!(!hits.is_empty());
    assert_eq!(hits[0].drawer_type, "drawer");
}

#[test]
fn rule_based_kg_extraction_adds_high_confidence_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let candidates = artifacts::extract_kg_candidates(
        "Kai prefers Clerk and Kai uses Rust. Maya works on Driftwood.",
    );
    let kg = mempalace_rust::kg::KnowledgeGraph::open(dir.path()).expect("open kg");
    for candidate in candidates
        .into_iter()
        .filter(|candidate| candidate.confidence >= 0.8)
    {
        kg.add_triple(
            &candidate.subject,
            &candidate.predicate,
            &candidate.object,
            None,
            None,
            candidate.confidence,
            None,
            None,
        )
        .expect("add triple");
    }
    let facts = kg.query_entity("Kai", None, "both").expect("query entity");
    assert!(facts.iter().any(|fact| fact.predicate == "prefers"));
}
