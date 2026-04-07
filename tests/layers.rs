mod common;

use mempalace_rust::layers;
use mempalace_rust::storage::{DrawerInput, Storage};

#[test]
fn layer_stack_formats_like_python_shape() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_dir = dir.path().join("config-home");
    let palace = dir.path().join("palace");
    let storage = Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(DrawerInput {
            id: "l1-a",
            wing: "wing_app",
            room: "auth",
            source_file: "auth.md",
            chunk_index: 0,
            added_by: "tester",
            content: "We switched auth providers after deciding Clerk simplified onboarding.",
            hall: Some("hall_facts"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("ha"),
            importance: Some(9.0),
            emotional_weight: None,
            weight: None,
        })
        .expect("add l1-a");

    let app_config = common::app_config(&config_dir, &palace);
    let stack = layers::MemoryStack::new(&app_config, &storage, Some("wing_app"));
    let wake = stack.wake_up().expect("wake up");
    assert!(wake.contains("## L1 — ESSENTIAL STORY"));
    assert!(wake.contains("[auth]"));
}
