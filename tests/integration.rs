use std::fs;
use tempfile::tempdir;

#[path = "../src/artifacts.rs"]
mod artifacts;
#[path = "../src/compression.rs"]
mod compression;
#[path = "../src/config.rs"]
mod config;
#[path = "../src/convo.rs"]
mod convo;
#[path = "../src/dialect.rs"]
mod dialect;
#[path = "../src/embedding.rs"]
mod embedding;
#[path = "../src/extractor.rs"]
mod extractor;
#[path = "../src/graph.rs"]
mod graph;
#[path = "../src/kg.rs"]
mod kg;
#[path = "../src/layers.rs"]
mod layers;
#[path = "../src/project.rs"]
mod project;
#[path = "../src/search.rs"]
mod search;
#[path = "../src/storage.rs"]
mod storage;

#[test]
fn project_mining_and_search_work() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("app");
    fs::create_dir_all(project_dir.join("backend")).unwrap();
    fs::write(
        project_dir.join("backend/app.py"),
        "def main():\n    print('hello world')\n# graphql migration decision\n".repeat(20),
    )
    .unwrap();
    fs::write(
        project_dir.join("mempalace.yaml"),
        "wing: test_project\nrooms:\n  - name: backend\n  - name: general\n",
    )
    .unwrap();

    let palace = dir.path().join("palace");
    let mut storage = storage::Storage::open(&palace).unwrap();
    let summary =
        project::mine_project(&project_dir, &mut storage, None, "tester", None, false).unwrap();
    assert!(summary.drawers_added > 0);

    let hits = storage.search("graphql migration", None, None, 5).unwrap();
    assert!(!hits.is_empty());
}

#[test]
fn hybrid_search_prefers_phrase_match_and_sets_backend() {
    let dir = tempdir().unwrap();
    let palace = dir.path().join("palace");
    let storage = storage::Storage::open(&palace).unwrap();

    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();

    let hits = storage
        .search("switch from REST to GraphQL", None, None, 5)
        .unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].id, "d1");
    assert_eq!(hits[0].embedding_backend, "strong_local");
    assert!(hits[0].semantic_score >= 0.0);
    assert!(hits[0].heuristic_score >= 0.0);
}

#[test]
fn fallback_search_still_works_for_punctuation_heavy_query() {
    let dir = tempdir().unwrap();
    let palace = dir.path().join("palace");
    let storage = storage::Storage::open(&palace).unwrap();

    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();

    let hits = storage.search("!!! ???", None, None, 5).unwrap();
    assert!(hits.iter().all(|hit| hit.lexical_score == 0.0));
    assert!(
        hits.iter()
            .all(|hit| hit.embedding_backend == "lexical_fallback"
                || hit.embedding_backend.is_empty())
    );

    let duplicate = storage
        .check_duplicate("Memory is persistence and continuity", 0.1)
        .unwrap();
    assert_eq!(
        duplicate.get("is_duplicate").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn layer_stack_formats_like_python_shape() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("config-home");
    std::fs::create_dir_all(&config_dir).unwrap();
    let identity_path = config_dir.join("identity.txt");
    std::fs::write(&identity_path, "I am Atlas, memory-aware.").unwrap();

    let palace = dir.path().join("palace");
    let storage = storage::Storage::open(&palace).unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();
    storage
        .add_drawer(storage::DrawerInput {
            id: "l1-b",
            wing: "wing_app",
            room: "deploy",
            source_file: "deploy.md",
            chunk_index: 0,
            added_by: "tester",
            content: "Deployment stabilized after fixing the CI pipeline and container startup ordering.",
            hall: Some("hall_events"),
            date: None,
            drawer_type: "drawer",
            source_hash: Some("hb"),
            importance: None,
            emotional_weight: Some(7.0),
            weight: None,
        })
        .unwrap();

    let app_config = config::AppConfig {
        config_dir: config_dir.clone(),
        config_file: config_dir.join("config.json"),
        identity_file: identity_path,
        palace_path: palace,
        collection_name: "mempalace_drawers".to_string(),
        people_map: std::collections::HashMap::new(),
        embedding_backend: "strong_local".to_string(),
    };

    let stack = layers::MemoryStack::new(&app_config, &storage, Some("wing_app"));
    let wake = stack.wake_up().unwrap();
    assert!(wake.contains("I am Atlas, memory-aware."));
    assert!(wake.contains("## L1 — ESSENTIAL STORY"));
    assert!(wake.contains("[auth]"));

    let recall = stack.recall(Some("wing_app"), Some("auth"), 5).unwrap();
    assert!(recall.starts_with("## L2 — ON-DEMAND"));
    assert!(recall.contains("[auth]"));

    let deep = stack
        .search("Clerk onboarding", Some("wing_app"), None, 5)
        .unwrap();
    assert!(deep.starts_with("## L3 — SEARCH RESULTS for \"Clerk onboarding\""));
}

#[test]
fn artifact_aware_retrieval_boosts_raw_parent_hit() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("artifact-app");
    fs::create_dir_all(project_dir.join("docs")).unwrap();
    fs::write(
        project_dir.join("docs/decisions.md"),
        "Kai prefers Clerk. The team decided to switch to Clerk because onboarding and DX were better.",
    )
    .unwrap();
    fs::write(
        project_dir.join("mempalace.yaml"),
        "wing: artifact_app\nrooms:\n  - name: docs\n  - name: general\n",
    )
    .unwrap();

    let palace = dir.path().join("palace");
    let mut storage = storage::Storage::open(&palace).unwrap();
    let summary =
        project::mine_project(&project_dir, &mut storage, None, "tester", None, false).unwrap();
    assert!(summary.drawers_added > 0);

    let hits = storage.search("Kai prefers Clerk", None, None, 5).unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].drawer_type, "drawer");
    assert!(hits.iter().any(|h| h.drawer_type == "drawer"));
}

#[test]
fn rule_based_kg_extraction_adds_high_confidence_facts() {
    let dir = tempdir().unwrap();
    let candidates = artifacts::extract_kg_candidates(
        "Kai prefers Clerk and Kai uses Rust. Maya works on Driftwood.",
    );
    assert!(candidates
        .iter()
        .any(|c| c.subject == "Kai" && c.predicate == "prefers" && c.object == "Clerk"));
    assert!(candidates
        .iter()
        .any(|c| c.subject == "Kai" && c.predicate == "uses" && c.object == "Rust"));
    assert!(candidates
        .iter()
        .any(|c| c.subject == "Maya" && c.predicate == "works_on" && c.object == "Driftwood"));

    let kg = kg::KnowledgeGraph::open(dir.path()).unwrap();
    for candidate in candidates {
        if candidate.confidence >= 0.8 {
            let _ = kg
                .add_triple(
                    &candidate.subject,
                    &candidate.predicate,
                    &candidate.object,
                    None,
                    None,
                    candidate.confidence,
                    None,
                    None,
                )
                .unwrap();
        }
    }
    let facts = kg.query_entity("Kai", None, "both").unwrap();
    assert!(facts.iter().any(|f| f.predicate == "prefers"));
}

#[test]
fn graph_metadata_enrichment_supports_tunnels_and_dates() {
    let dir = tempdir().unwrap();
    let palace = dir.path().join("palace");
    let storage = storage::Storage::open(&palace).unwrap();

    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();

    let tunnels = graph::find_tunnels(&storage, Some("alpha"), Some("beta")).unwrap();
    let arr = tunnels.as_array().unwrap();
    assert!(arr
        .iter()
        .any(|t| t.get("room").and_then(|v| v.as_str()) == Some("architecture")));
    let architecture = arr
        .iter()
        .find(|t| t.get("room").and_then(|v| v.as_str()) == Some("architecture"))
        .unwrap();
    assert_eq!(
        architecture.get("recent").and_then(|v| v.as_str()),
        Some("2026-03-11")
    );
    assert!(architecture.get("wing_halls").is_some());
    assert!(architecture.get("drawer_types").is_some());

    let traversal = graph::traverse(&storage, "architecture", 1).unwrap();
    let nodes = traversal.as_array().unwrap();
    assert_eq!(
        nodes[0].get("room").and_then(|v| v.as_str()),
        Some("architecture")
    );
    assert!(nodes[0].get("dates").is_some());
}

#[test]
fn ingestion_populates_date_and_hall_metadata() {
    let dir = tempdir().unwrap();
    let project_dir = dir.path().join("roadmap-app");
    fs::create_dir_all(project_dir.join("planning")).unwrap();
    fs::write(
        project_dir.join("planning/2026-04-01-roadmap.md"),
        "Roadmap update on 2026-04-01. Milestone review and next step planning.",
    )
    .unwrap();
    fs::write(
        project_dir.join("mempalace.yaml"),
        "wing: roadmap_app\nrooms:\n  - name: planning\n  - name: general\n",
    )
    .unwrap();

    let palace = dir.path().join("palace-project");
    let mut storage = storage::Storage::open(&palace).unwrap();
    project::mine_project(&project_dir, &mut storage, None, "tester", None, false).unwrap();
    let planning = storage
        .scoped_drawers(Some("roadmap_app"), Some("planning"), 10)
        .unwrap();
    assert!(!planning.is_empty());
    assert!(planning
        .iter()
        .any(|d| d.date.as_deref() == Some("2026-04-01")));
    assert!(planning
        .iter()
        .all(|d| d.hall.as_deref() == Some("hall_events")));

    let convos_dir = dir.path().join("convos");
    fs::create_dir_all(&convos_dir).unwrap();
    fs::write(
        convos_dir.join("2026-04-02-chat.txt"),
        "> We decided to switch auth providers because onboarding was better.\nClerk looked cleaner.\n\n> Great, log it.\nDone.\n",
    )
    .unwrap();
    let palace2 = dir.path().join("palace-convo");
    let mut storage2 = storage::Storage::open(&palace2).unwrap();
    convo::mine_conversations(
        &convos_dir,
        &mut storage2,
        Some("chatwing"),
        "tester",
        None,
        false,
        "exchange",
    )
    .unwrap();
    let drawers = storage2.sample_for_wing(Some("chatwing"), 20).unwrap();
    assert!(drawers
        .iter()
        .any(|d| d.date.as_deref() == Some("2026-04-02")));
    assert!(drawers
        .iter()
        .any(|d| d.hall.as_deref() == Some("hall_facts")));
}

#[test]
fn compression_artifacts_are_maintained_and_retrievable() {
    let dir = tempdir().unwrap();
    let palace = dir.path().join("palace-aaak");
    let mut storage = storage::Storage::open(&palace).unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();

    let stats =
        compression::maintain_compressed_artifacts(&mut storage, Some("alpha"), false).unwrap();
    assert!(stats.artifacts_written >= 1);

    let compressed = storage.find_compressed_for_raw("raw-aaak-1").unwrap();
    assert!(compressed.is_some());
    assert_eq!(compressed.as_ref().unwrap().drawer_type, "compressed");

    let hits = storage
        .search("Clerk onboarding friction", Some("alpha"), None, 5)
        .unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].drawer_type, "drawer");
}

#[test]
fn layer1_can_prefer_compressed_snippets() {
    let dir = tempdir().unwrap();
    let palace = dir.path().join("palace-l1-aaak");
    let storage = storage::Storage::open(&palace).unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();
    storage
        .add_drawer(storage::DrawerInput {
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
        .unwrap();

    std::env::set_var("MEMPALACE_PREFER_COMPRESSED", "1");
    let config_dir = dir.path().join("cfg");
    std::fs::create_dir_all(&config_dir).unwrap();
    let identity_path = config_dir.join("identity.txt");
    std::fs::write(&identity_path, "identity").unwrap();
    let app_config = config::AppConfig {
        config_dir: config_dir.clone(),
        config_file: config_dir.join("config.json"),
        identity_file: identity_path,
        palace_path: palace,
        collection_name: "mempalace_drawers".to_string(),
        people_map: std::collections::HashMap::new(),
        embedding_backend: "strong_local".to_string(),
    };
    let stack = layers::MemoryStack::new(&app_config, &storage, Some("alpha"));
    let wake = stack.wake_up().unwrap();
    assert!(wake.contains("0:???|auth_onboarding_burden") || wake.contains("DECISION+TECHNICAL"));
    std::env::remove_var("MEMPALACE_PREFER_COMPRESSED");
}

#[test]
fn convo_mining_normalizes_and_indexes() {
    let dir = tempdir().unwrap();
    let convos_dir = dir.path().join("convos");
    fs::create_dir_all(&convos_dir).unwrap();
    fs::write(
        convos_dir.join("chat.txt"),
        "> What is memory?\nMemory is persistence.\n\n> Why does it matter?\nIt enables continuity.\n\n> How do we build it?\nWith structured storage.\n",
    )
    .unwrap();

    let palace = dir.path().join("palace");
    let mut storage = storage::Storage::open(&palace).unwrap();
    let summary = convo::mine_conversations(
        &convos_dir,
        &mut storage,
        Some("test_convos"),
        "tester",
        None,
        false,
        "exchange",
    )
    .unwrap();
    assert!(summary.drawers_added >= 2);

    let hits = storage.search("memory persistence", None, None, 5).unwrap();
    assert!(!hits.is_empty());
}
