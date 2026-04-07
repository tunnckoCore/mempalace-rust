use crate::config::AppConfig;
use crate::dialect::AAAK_SPEC;
use crate::graph;
use crate::kg::KnowledgeGraph;
use crate::limits::{MAX_MCP_MESSAGE_BYTES, MAX_QUERY_CHARS};
use crate::search::normalize_query_for_fts;
use crate::storage::{DrawerInput, Storage};
use anyhow::{ensure, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::io::{self, BufRead, BufReader, Write};

const PALACE_PROTOCOL: &str = "IMPORTANT — MemPalace Memory Protocol:\n1. ON WAKE-UP: Call mempalace_status to load palace overview + AAAK spec.\n2. BEFORE RESPONDING about any person, project, or past event: call mempalace_kg_query or mempalace_search FIRST. Never guess — verify.\n3. IF UNSURE about a fact: say 'let me check' and query the palace.\n4. AFTER EACH SESSION: call mempalace_diary_write to record what happened.\n5. WHEN FACTS CHANGE: call mempalace_kg_invalidate on the old fact, mempalace_kg_add for the new one.";
const MUTATING_TOOLS: &[&str] = &[
    "mempalace_add_drawer",
    "mempalace_delete_drawer",
    "mempalace_kg_add",
    "mempalace_kg_invalidate",
    "mempalace_diary_write",
];

fn read_stdio_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>> {
    let mut first_line = String::new();
    let bytes = reader.read_line(&mut first_line)?;
    if bytes == 0 {
        return Ok(None);
    }
    if first_line
        .to_ascii_lowercase()
        .starts_with("content-length:")
    {
        let len = first_line
            .split(':')
            .nth(1)
            .map(str::trim)
            .ok_or_else(|| anyhow::anyhow!("invalid Content-Length header"))?
            .parse::<usize>()?;
        if len > MAX_MCP_MESSAGE_BYTES {
            anyhow::bail!("Content-Length exceeds maximum allowed size");
        }
        let mut line = String::new();
        loop {
            line.clear();
            let read = reader.read_line(&mut line)?;
            if read == 0 || line == "\r\n" || line == "\n" {
                break;
            }
        }
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf)?;
        let req: Value = serde_json::from_slice(&buf)?;
        Ok(Some(req))
    } else {
        let line = first_line.trim();
        if line.len() > MAX_MCP_MESSAGE_BYTES {
            anyhow::bail!("JSON line exceeds maximum allowed size");
        }
        if line.is_empty() {
            return Ok(Some(json!({})));
        }
        let req: Value = serde_json::from_str(line)?;
        Ok(Some(req))
    }
}

fn write_stdio_message<W: Write>(writer: &mut W, resp: &Value) -> Result<()> {
    let body = serde_json::to_vec(resp)?;
    writer.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

pub fn run_stdio_server(config: &AppConfig) -> Result<()> {
    eprintln!(
        "mempalace-rust MCP ready (transport=stdio, backend={}, local_provider={})",
        config.embedding_backend, config.local_embedding_provider
    );
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = io::stdout();

    while let Some(req) = read_stdio_message(&mut reader)? {
        let resp = handle_request(config, req)?;
        if !resp.is_null() {
            write_stdio_message(&mut stdout, &resp)?;
        }
    }
    Ok(())
}

fn handle_request(config: &AppConfig, req: Value) -> Result<Value> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    match method {
        "initialize" => Ok(
            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"mempalace-rust","version":"0.2.0"},"capabilities":{"tools":{}}}}),
        ),
        "tools/list" => Ok(json!({"jsonrpc":"2.0","id":id,"result":{"tools": tool_inventory()}})),
        "tools/call" => {
            let name = req
                .get("params")
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = req
                .get("params")
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            let result = call_tool(config, name, &args)?;
            Ok(
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":serde_json::to_string_pretty(&result)?}]}}),
            )
        }
        "notifications/initialized" => Ok(Value::Null),
        _ => Ok(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":format!("Unknown method: {}", method)}}),
        ),
    }
}

fn tool_inventory() -> Vec<Value> {
    let allow_mutations = mutations_enabled();
    let mut tools = vec![
        tool(
            "mempalace_status",
            "Palace overview — total drawers, wing and room counts",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_list_wings",
            "List all wings with drawer counts",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_list_rooms",
            "List rooms within a wing (or all rooms if no wing given)",
            json!({"type":"object","properties":{"wing":{"type":"string"}}}),
        ),
        tool(
            "mempalace_get_taxonomy",
            "Full taxonomy: wing → room → drawer count",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_get_aaak_spec",
            "Get the AAAK dialect specification",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_get_compact_context",
            "Get compact AAAK-aware context for a wing or room",
            json!({"type":"object","properties":{"wing":{"type":"string"},"room":{"type":"string"},"limit":{"type":"integer"}}}),
        ),
        tool(
            "mempalace_search",
            "Hybrid search, optional wing/room filter",
            json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"},"wing":{"type":"string"},"room":{"type":"string"}},"required":["query"]}),
        ),
        tool(
            "mempalace_check_duplicate",
            "Check if content already exists before filing",
            json!({"type":"object","properties":{"content":{"type":"string"},"threshold":{"type":"number"}},"required":["content"]}),
        ),
        tool(
            "mempalace_add_drawer",
            "File verbatim content into a wing/room",
            json!({"type":"object","properties":{"wing":{"type":"string"},"room":{"type":"string"},"content":{"type":"string"},"source_file":{"type":"string"},"added_by":{"type":"string"}},"required":["wing","room","content"]}),
        ),
        tool(
            "mempalace_delete_drawer",
            "Delete a drawer by ID",
            json!({"type":"object","properties":{"drawer_id":{"type":"string"}},"required":["drawer_id"]}),
        ),
        tool(
            "mempalace_kg_query",
            "Query the knowledge graph for an entity's relationships",
            json!({"type":"object","properties":{"entity":{"type":"string"},"as_of":{"type":"string"},"direction":{"type":"string"}},"required":["entity"]}),
        ),
        tool(
            "mempalace_kg_add",
            "Add a fact to the knowledge graph",
            json!({"type":"object","properties":{"subject":{"type":"string"},"predicate":{"type":"string"},"object":{"type":"string"},"valid_from":{"type":"string"},"source_closet":{"type":"string"}},"required":["subject","predicate","object"]}),
        ),
        tool(
            "mempalace_kg_invalidate",
            "Mark a fact as no longer true",
            json!({"type":"object","properties":{"subject":{"type":"string"},"predicate":{"type":"string"},"object":{"type":"string"},"ended":{"type":"string"}},"required":["subject","predicate","object"]}),
        ),
        tool(
            "mempalace_kg_timeline",
            "Chronological timeline of facts",
            json!({"type":"object","properties":{"entity":{"type":"string"}}}),
        ),
        tool(
            "mempalace_kg_stats",
            "Knowledge graph overview",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_traverse",
            "Walk the palace graph from a room",
            json!({"type":"object","properties":{"start_room":{"type":"string"},"max_hops":{"type":"integer"}},"required":["start_room"]}),
        ),
        tool(
            "mempalace_find_tunnels",
            "Find rooms that bridge two wings",
            json!({"type":"object","properties":{"wing_a":{"type":"string"},"wing_b":{"type":"string"}}}),
        ),
        tool(
            "mempalace_graph_stats",
            "Palace graph overview",
            json!({"type":"object","properties":{}}),
        ),
        tool(
            "mempalace_diary_write",
            "Write a diary entry for this agent",
            json!({"type":"object","properties":{"agent_name":{"type":"string"},"entry":{"type":"string"},"topic":{"type":"string"}},"required":["agent_name","entry"]}),
        ),
        tool(
            "mempalace_diary_read",
            "Read an agent's recent diary entries",
            json!({"type":"object","properties":{"agent_name":{"type":"string"},"last_n":{"type":"integer"}},"required":["agent_name"]}),
        ),
    ];
    if !allow_mutations {
        tools.retain(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(|name| !MUTATING_TOOLS.contains(&name))
                .unwrap_or(true)
        });
    }
    tools
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({"name":name,"description":description,"inputSchema":input_schema})
}

pub fn handle_test_request(config: &AppConfig, req: Value) -> Result<Value> {
    handle_request(config, req)
}

fn mutations_enabled() -> bool {
    std::env::var("MEMPALACE_ENABLE_MUTATIONS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn require_mutations_enabled(name: &str) -> Result<()> {
    if MUTATING_TOOLS.contains(&name) && !mutations_enabled() {
        anyhow::bail!(
            "tool '{}' is disabled unless MEMPALACE_ENABLE_MUTATIONS=1",
            name
        );
    }
    Ok(())
}

fn generate_mutation_id(prefix: &str) -> String {
    format!(
        "{}_{}_{:x}",
        prefix,
        Utc::now().timestamp_millis(),
        md5::compute(format!(
            "{}:{}",
            prefix,
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    )
}

fn call_tool(config: &AppConfig, name: &str, args: &Value) -> Result<Value> {
    require_mutations_enabled(name)?;
    let storage = Storage::open(&config.palace_path)?;
    let kg = KnowledgeGraph::open(&config.config_dir)?;
    Ok(match name {
        "mempalace_status" => {
            let status = storage.status()?;
            json!({"total_drawers": status.total_drawers, "wings": status.by_wing.keys().collect::<Vec<_>>(), "rooms": status.by_wing, "palace_path": config.palace_path, "protocol": PALACE_PROTOCOL, "aaak_dialect": AAAK_SPEC, "artifact_types": status.artifacts_by_type, "compact_context_available": true, "embedding_backend": config.embedding_backend, "local_embedding_provider": config.local_embedding_provider, "openai_embedding_model": config.openai_embedding_model, "openai_base_url": config.openai_base_url})
        }
        "mempalace_list_wings" => json!({"wings": storage.top_wings(100)?}),
        "mempalace_list_rooms" => {
            let status = storage.status()?;
            if let Some(wing) = args.get("wing").and_then(|v| v.as_str()) {
                json!({"wing": wing, "rooms": status.by_wing.get(wing)})
            } else {
                json!({"rooms": status.by_wing})
            }
        }
        "mempalace_get_taxonomy" => storage.taxonomy()?,
        "mempalace_get_aaak_spec" => json!({"aaak_spec": AAAK_SPEC}),
        "mempalace_get_compact_context" => {
            let wing = args.get("wing").and_then(|v| v.as_str());
            let room = args.get("room").and_then(|v| v.as_str());
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 50) as usize;
            let drawers = storage.scoped_drawers(wing, room, limit)?;
            let compact: Vec<_> = drawers
                .into_iter()
                .take(limit)
                .map(|d| {
                    let compressed = storage.find_compressed_for_raw(&d.id).ok().flatten();
                    json!({
                        "id": d.id,
                        "wing": d.wing,
                        "room": d.room,
                        "source_file": d.source_file,
                        "date": d.date,
                        "raw": d.content,
                        "compressed": compressed.as_ref().map(|c| c.content.clone()).unwrap_or_default(),
                    })
                })
                .collect();
            json!({"items": compact, "aaak_spec": AAAK_SPEC})
        }
        "mempalace_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(5)
                .clamp(1, 50) as usize;
            let wing = args.get("wing").and_then(|v| v.as_str());
            let room = args.get("room").and_then(|v| v.as_str());
            if query.chars().count() > MAX_QUERY_CHARS {
                json!({"error":"query exceeds max length"})
            } else if normalize_query_for_fts(query).is_none() {
                json!({"error":"query must contain at least one alphanumeric token with length >= 2"})
            } else {
                json!({"query": query, "results": storage.search_hybrid(query, wing, room, limit)?})
            }
        }
        "mempalace_check_duplicate" => {
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let threshold = args
                .get("threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.85)
                .clamp(0.0, 1.0);
            if content.trim().is_empty() {
                json!({"error": "content must not be empty"})
            } else {
                storage.check_duplicate(content, threshold)?
            }
        }
        "mempalace_add_drawer" => {
            let wing = args
                .get("wing")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();
            let room = args
                .get("room")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim();
            let source_file = args
                .get("source_file")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let added_by = args
                .get("added_by")
                .and_then(|v| v.as_str())
                .unwrap_or("mcp");
            if wing.is_empty() || room.is_empty() || content.is_empty() {
                json!({"success": false, "error": "wing, room, and content are required and must not be empty"})
            } else if content.chars().count() > MAX_QUERY_CHARS {
                json!({"success": false, "error": "content exceeds max length"})
            } else {
                let dup = storage.check_duplicate(content, 0.85)?;
                if dup.get("is_duplicate").and_then(|v| v.as_bool()) == Some(true) {
                    json!({"success": false, "reason": "duplicate", "matches": dup.get("matches")})
                } else {
                    let id = generate_mutation_id(&format!("drawer_{}_{}", wing, room));
                    storage.add_drawer(DrawerInput {
                        id: &id,
                        wing,
                        room,
                        source_file,
                        chunk_index: 0,
                        added_by,
                        content,
                        hall: None,
                        date: None,
                        drawer_type: "drawer",
                        source_hash: None,
                        importance: None,
                        emotional_weight: None,
                        weight: None,
                    })?;
                    json!({"success": true, "drawer_id": id, "wing": wing, "room": room})
                }
            }
        }
        "mempalace_delete_drawer" => {
            let drawer_id = args
                .get("drawer_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            json!({"success": storage.delete_drawer(drawer_id)?, "drawer_id": drawer_id})
        }
        "mempalace_kg_query" => {
            let entity = args
                .get("entity")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            ensure!(!entity.trim().is_empty(), "entity is required");
            let as_of = args.get("as_of").and_then(|v| v.as_str());
            let direction = args
                .get("direction")
                .and_then(|v| v.as_str())
                .unwrap_or("both");
            let facts = kg.query_entity(entity, as_of, direction)?;
            json!({"entity": entity, "as_of": as_of, "facts": facts, "count": facts.len()})
        }
        "mempalace_kg_add" => {
            let subject = args
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let predicate = args
                .get("predicate")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let object = args
                .get("object")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            ensure!(
                !subject.trim().is_empty()
                    && !predicate.trim().is_empty()
                    && !object.trim().is_empty(),
                "subject, predicate, and object are required"
            );
            let valid_from = args.get("valid_from").and_then(|v| v.as_str());
            let source_closet = args.get("source_closet").and_then(|v| v.as_str());
            let triple_id = kg.add_triple(
                subject,
                predicate,
                object,
                valid_from,
                None,
                1.0,
                source_closet,
                None,
            )?;
            json!({"success": true, "triple_id": triple_id})
        }
        "mempalace_kg_invalidate" => {
            let subject = args
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let predicate = args
                .get("predicate")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let object = args
                .get("object")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            ensure!(
                !subject.trim().is_empty()
                    && !predicate.trim().is_empty()
                    && !object.trim().is_empty(),
                "subject, predicate, and object are required"
            );
            let ended = args.get("ended").and_then(|v| v.as_str());
            kg.invalidate(subject, predicate, object, ended)?;
            json!({"success": true})
        }
        "mempalace_kg_timeline" => {
            let entity = args.get("entity").and_then(|v| v.as_str());
            json!({"timeline": kg.timeline(entity)?})
        }
        "mempalace_kg_stats" => kg.stats()?,
        "mempalace_traverse" => {
            let start_room = args
                .get("start_room")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let max_hops = args
                .get("max_hops")
                .and_then(|v| v.as_u64())
                .unwrap_or(2)
                .clamp(1, 6) as usize;
            graph::traverse(&storage, start_room, max_hops)?
        }
        "mempalace_find_tunnels" => {
            let wing_a = args.get("wing_a").and_then(|v| v.as_str());
            let wing_b = args.get("wing_b").and_then(|v| v.as_str());
            graph::find_tunnels(&storage, wing_a, wing_b)?
        }
        "mempalace_graph_stats" => graph::graph_stats(&storage)?,
        "mempalace_diary_write" => {
            let agent_name = args
                .get("agent_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let entry = args
                .get("entry")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let topic = args
                .get("topic")
                .and_then(|v| v.as_str())
                .unwrap_or("general");
            let wing = format!("wing_{}", agent_name.to_lowercase().replace(' ', "_"));
            let room = "diary";
            let now = Utc::now();
            let entry_id = generate_mutation_id(&format!("diary_{}", wing));
            storage.add_drawer(DrawerInput {
                id: &entry_id,
                wing: &wing,
                room,
                source_file: "",
                chunk_index: 0,
                added_by: agent_name,
                content: entry,
                hall: Some("hall_diary"),
                date: Some(&now.date_naive().to_string()),
                drawer_type: "diary_entry",
                source_hash: None,
                importance: None,
                emotional_weight: None,
                weight: None,
            })?;
            json!({"success": true, "entry_id": entry_id, "agent": agent_name, "topic": topic, "timestamp": now.to_rfc3339()})
        }
        "mempalace_diary_read" => {
            let agent_name = args
                .get("agent_name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let last_n = args
                .get("last_n")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .clamp(1, 100) as usize;
            let wing = format!("wing_{}", agent_name.to_lowercase().replace(' ', "_"));
            let entries = storage.sample_for_wing(Some(&wing), last_n)?;
            let entries: Vec<_> = entries.into_iter().filter(|d| d.room == "diary").map(|d| json!({"date": d.date, "timestamp": d.filed_at, "topic": "general", "content": d.content})).collect();
            json!({"agent": agent_name, "entries": entries, "showing": entries.len()})
        }
        _ => json!({"error": format!("unknown tool {}", name)}),
    })
}
