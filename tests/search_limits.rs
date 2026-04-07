#[test]
fn long_query_semantic_fallback_still_returns_without_blowing_up() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace");
    let storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(mempalace_rust::storage::DrawerInput {
            id: "s1",
            wing: "alpha",
            room: "general",
            source_file: "doc.md",
            chunk_index: 0,
            added_by: "tester",
            content: "Memory persistence continuity storage sessions identity graph retrieval",
            hall: Some("hall_facts"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("sh1"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add drawer");

    let long_query = "x".repeat(1500);
    let results = storage
        .search(&long_query, None, None, 5)
        .expect("search long query");
    assert!(results.len() <= 5);
}
