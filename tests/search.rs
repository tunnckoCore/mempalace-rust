mod common;

use mempalace_rust::storage::{DrawerInput, Storage};

#[test]
fn hybrid_search_prefers_phrase_match_and_sets_backend() {
    let (_dir, palace) = common::temp_palace();
    let storage = Storage::open(&palace).expect("open storage");

    storage
        .add_drawer(DrawerInput {
            id: "d1",
            wing: "test",
            room: "architecture",
            source_file: "doc1.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We decided to switch from REST to GraphQL because it simplified typed client queries.",
            hall: Some("hall_facts"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("h1"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add d1");
    storage
        .add_drawer(DrawerInput {
            id: "d2",
            wing: "test",
            room: "general",
            source_file: "doc2.md",
            chunk_index: 0,
            added_by: "tester",
            content:
                "We discussed APIs and data layers in general terms, including transport options.",
            hall: Some("hall_events"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("h2"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add d2");

    let hits = storage
        .search("switch from REST to GraphQL", None, None, 5)
        .expect("search");
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "d1");
    assert_eq!(hits[0].embedding_backend, "local_builtin");
}

#[test]
fn fallback_search_still_works_for_punctuation_heavy_query() {
    let (_dir, palace) = common::temp_palace();
    let storage = Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(DrawerInput {
            id: "d3",
            wing: "test",
            room: "general",
            source_file: "doc3.md",
            chunk_index: 0,
            added_by: "tester",
            content: "Memory is persistence and continuity across sessions.",
            hall: Some("hall_events"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("h3"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add d3");

    let hits = storage.search("!!! ???", None, None, 5).expect("search");
    assert!(hits.iter().all(|hit| hit.lexical_score == 0.0));
}
