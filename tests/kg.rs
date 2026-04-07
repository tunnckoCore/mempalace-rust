#[test]
fn rule_based_kg_extraction_adds_high_confidence_facts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let candidates = mempalace_rust::artifacts::extract_kg_candidates(
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
    let facts = kg.query_entity("Kai", None, "both").expect("query facts");
    assert!(facts.iter().any(|fact| fact.predicate == "prefers"));
}
