mod common;

use mempalace_rust::storage::{DrawerInput, Storage};

#[test]
fn graph_metadata_enrichment_supports_tunnels_and_dates() {
    let (_dir, palace) = common::temp_palace();
    let storage = Storage::open(&palace).expect("open storage");

    storage
        .add_drawer(DrawerInput {
            id: "g1",
            wing: "alpha",
            room: "architecture",
            source_file: "alpha/2026-03-10-arch.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We decided on the auth architecture.",
            hall: Some("hall_facts"),
            date: Some("2026-03-10"),
            drawer_type: "drawer",
            source_hash: Some("gh1"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add g1");
    storage
        .add_drawer(DrawerInput {
            id: "g2",
            wing: "beta",
            room: "architecture",
            source_file: "beta/2026-03-11-arch.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We documented the architecture migration.",
            hall: Some("hall_facts"),
            date: Some("2026-03-11"),
            drawer_type: "decision_bridge",
            source_hash: Some("gh2"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add g2");

    let tunnels = mempalace_rust::graph::find_tunnels(&storage, Some("alpha"), Some("beta"))
        .expect("find tunnels");
    let arr = tunnels.as_array().expect("array");
    assert!(arr
        .iter()
        .any(|t| t.get("room").and_then(|v| v.as_str()) == Some("architecture")));
}
