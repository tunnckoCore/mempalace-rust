mod common;

use mempalace_rust::compression;
use mempalace_rust::layers;
use mempalace_rust::storage::{DrawerInput, Storage};

#[test]
fn compression_artifacts_are_maintained_and_retrievable() {
    let (_dir, palace) = common::temp_palace();
    let mut storage = Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(DrawerInput {
            id: "raw-aaak-1",
            wing: "alpha",
            room: "architecture",
            source_file: "notes/arch.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We decided to switch auth providers because Clerk improved onboarding and reduced friction.",
            hall: Some("hall_facts"),
            date: Some("2026-04-07"),
            drawer_type: "drawer",
            source_hash: Some("rawhash1"),
            importance: Some(8.0),
            emotional_weight: None,
            weight: None,
        })
        .expect("add drawer");

    let stats = compression::maintain_compressed_artifacts(&mut storage, Some("alpha"), false)
        .expect("maintain compressed artifacts");
    assert!(stats.artifacts_written >= 1);
    let compressed = storage
        .find_compressed_for_raw("raw-aaak-1")
        .expect("find compressed")
        .expect("compressed drawer");
    assert_eq!(compressed.drawer_type, "compressed");
    assert!(compressed.source_file.ends_with("#aaak"));
}

#[test]
fn layer1_can_prefer_compressed_snippets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace-l1-aaak");
    let storage = Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(DrawerInput {
            id: "raw-aaak-2",
            wing: "alpha",
            room: "architecture",
            source_file: "notes/arch2.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We decided to migrate auth because the previous provider created onboarding friction and support burden.",
            hall: Some("hall_facts"),
            date: Some("2026-04-07"),
            drawer_type: "drawer",
            source_hash: Some("rawhash2"),
            importance: Some(9.0),
            emotional_weight: None,
            weight: None,
        })
        .expect("add raw drawer");
    storage
        .add_drawer(DrawerInput {
            id: "aaak_raw-aaak-2_123456789abc",
            wing: "alpha",
            room: "architecture",
            source_file: "notes/arch2.md#aaak",
            chunk_index: 0,
            added_by: "compress",
            content: "alpha|architecture|2026-04-07|arch2\n0:???|auth_onboarding_burden|\"We decided to migrate auth...\"|determ|DECISION+TECHNICAL",
            hall: Some("hall_facts"),
            date: Some("2026-04-07"),
            drawer_type: "compressed",
            source_hash: Some("aaakhash2"),
            importance: Some(7.5),
            emotional_weight: None,
            weight: Some(7.0),
        })
        .expect("add compressed drawer");

    std::env::set_var("MEMPALACE_PREFER_COMPRESSED", "1");
    let config_dir = dir.path().join("cfg");
    let app_config = common::app_config(&config_dir, &palace);
    let stack = layers::MemoryStack::new(&app_config, &storage, Some("alpha"));
    let wake = stack.wake_up().expect("wake up");
    assert!(wake.contains("DECISION+TECHNICAL"));
    std::env::remove_var("MEMPALACE_PREFER_COMPRESSED");
}
