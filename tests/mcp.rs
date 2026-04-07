mod common;

#[test]
fn mcp_compact_context_tool_returns_compact_payload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace");
    let config_root = dir.path().join("cfg");
    let config = common::app_config(&config_root, &palace);
    let storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(mempalace_rust::storage::DrawerInput {
            id: "raw-1",
            wing: "alpha",
            room: "architecture",
            source_file: "arch.md",
            chunk_index: 0,
            added_by: "tester",
            content: "raw memory",
            hall: Some("hall_facts"),
            date: Some("2026-04-07"),
            drawer_type: "drawer",
            source_hash: Some("h1"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add raw drawer");

    let req = serde_json::json!({
        "id": 1,
        "method": "tools/call",
        "params": {"name": "mempalace_get_compact_context", "arguments": {"wing": "alpha", "limit": 5}}
    });
    let response = mempalace_rust::mcp::handle_test_request(&config, req).expect("handle request");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text payload");
    assert!(text.contains("raw-1"));
}

#[test]
fn duplicate_threshold_is_clamped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace-threshold");
    let config_root = dir.path().join("cfg-threshold");
    let config = common::app_config(&config_root, &palace);
    let storage = mempalace_rust::storage::Storage::open(&palace).expect("open storage");
    storage
        .add_drawer(mempalace_rust::storage::DrawerInput {
            id: "dup-1",
            wing: "alpha",
            room: "general",
            source_file: "dup.md",
            chunk_index: 0,
            added_by: "tester",
            content: "Memory is persistence and continuity.",
            hall: Some("hall_facts"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("dup-h1"),
            importance: None,
            emotional_weight: None,
            weight: None,
        })
        .expect("add dup drawer");
    let req = serde_json::json!({
        "id": 3,
        "method": "tools/call",
        "params": {"name": "mempalace_check_duplicate", "arguments": {"content": "Memory is persistence and continuity.", "threshold": 99.0}}
    });
    let response = mempalace_rust::mcp::handle_test_request(&config, req).expect("handle request");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("payload text");
    assert!(text.contains("is_duplicate"));
}

#[test]
fn mutating_tools_are_gated_by_env() {
    let dir = tempfile::tempdir().expect("tempdir");
    let palace = dir.path().join("palace");
    let config_root = dir.path().join("cfg");
    let config = common::app_config(&config_root, &palace);
    std::env::remove_var("MEMPALACE_ENABLE_MUTATIONS");
    let req = serde_json::json!({
        "id": 2,
        "method": "tools/call",
        "params": {"name": "mempalace_add_drawer", "arguments": {"wing": "alpha", "room": "general", "content": "hello"}}
    });
    let error =
        mempalace_rust::mcp::handle_test_request(&config, req).expect_err("mutations disabled");
    assert!(error.to_string().contains("disabled"));
}
