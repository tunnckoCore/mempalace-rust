use crate::artifacts::{derive_convo_artifacts, extract_kg_candidates, infer_date};
use crate::extractor::extract_memories;
use crate::kg::KnowledgeGraph;
use crate::limits::{MAX_INGEST_CHARS, MAX_INGEST_FILE_BYTES};
use crate::search::slugify;
use crate::storage::Storage;
use crate::storage_types::{DrawerInputOwned, SourceRefreshPlanOwned};
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MIN_CHUNK_SIZE: usize = 30;
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    ".next",
    ".mempalace",
    "target",
];
const CONVO_EXTENSIONS: &[&str] = &[".txt", ".md", ".json", ".jsonl"];

#[derive(Debug, Clone)]
pub struct MineSummary {
    pub wing: String,
    pub files_seen: usize,
    pub files_skipped: usize,
    pub drawers_added: usize,
    pub room_counts: HashMap<String, usize>,
}

pub fn mine_conversations(
    convo_dir: &Path,
    storage: &mut Storage,
    wing_override: Option<&str>,
    agent: &str,
    limit: Option<usize>,
    dry_run: bool,
    extract_mode: &str,
) -> Result<MineSummary> {
    let wing = wing_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| slugify(&crate::project::infer_wing_name(convo_dir, "convos")));
    let files = scan_convos(convo_dir, limit)?;
    let mut summary = MineSummary {
        wing: wing.clone(),
        files_seen: files.len(),
        files_skipped: 0,
        drawers_added: 0,
        room_counts: HashMap::new(),
    };

    let kg = KnowledgeGraph::open(&crate::config::default_config_dir()?)?;

    for file in files {
        let source_file = file.to_string_lossy().to_string();
        let metadata = match fs::metadata(&file) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.len() > MAX_INGEST_FILE_BYTES {
            summary.files_skipped += 1;
            continue;
        }
        let content = match normalize(&file) {
            Ok(text) => text,
            Err(_) => continue,
        };
        if content.chars().count() > MAX_INGEST_CHARS {
            summary.files_skipped += 1;
            continue;
        }
        let source_hash = format!("{:x}", md5::compute(content.as_bytes()));
        if !dry_run && storage.source_is_current(&source_file, &source_hash)? {
            summary.files_skipped += 1;
            continue;
        }
        if content.trim().len() < MIN_CHUNK_SIZE {
            continue;
        }
        let inferred_date = infer_date(&content, Some(&source_file));
        let mut planned: Vec<DrawerInputOwned> = Vec::new();
        if extract_mode == "general" {
            let memories = extract_memories(&content, 0.3);
            if memories.is_empty() {
                continue;
            }
            for (idx, memory) in memories.iter().enumerate() {
                let room = memory.memory_type.clone();
                *summary.room_counts.entry(room.clone()).or_insert(0) += 1;
                if dry_run {
                    continue;
                }
                let digest = format!(
                    "{:x}",
                    md5::compute(format!("{}:{}:{}:general", source_file, idx, source_hash))
                );
                let drawer_id = format!("drawer_{}_{}_{}", wing, room, &digest[..16]);
                let hall = infer_convo_hall(&room, &memory.content, &source_file, true);
                planned.push(DrawerInputOwned {
                    id: drawer_id,
                    wing: wing.clone(),
                    room,
                    source_file: source_file.clone(),
                    chunk_index: idx as i64,
                    added_by: agent.to_string(),
                    content: memory.content.clone(),
                    hall: Some(hall),
                    date: inferred_date.clone(),
                    drawer_type: "derived_memory".to_string(),
                    source_hash: Some(source_hash.clone()),
                    importance: None,
                    emotional_weight: None,
                    weight: None,
                });
            }
        } else {
            let chunks = chunk_exchanges(&content);
            if chunks.is_empty() {
                continue;
            }
            for (idx, chunk) in chunks.iter().enumerate() {
                let room = detect_convo_room(chunk);
                *summary.room_counts.entry(room.clone()).or_insert(0) += 1;
                if dry_run {
                    continue;
                }
                let digest = format!(
                    "{:x}",
                    md5::compute(format!("{}:{}:{}", source_file, idx, source_hash))
                );
                let drawer_id = format!("drawer_{}_{}_{}", wing, room, &digest[..16]);
                let hall = infer_convo_hall(&room, chunk, &source_file, false);
                planned.push(DrawerInputOwned {
                    id: drawer_id,
                    wing: wing.clone(),
                    room,
                    source_file: source_file.clone(),
                    chunk_index: idx as i64,
                    added_by: agent.to_string(),
                    content: chunk.clone(),
                    hall: Some(hall),
                    date: inferred_date.clone(),
                    drawer_type: "drawer".to_string(),
                    source_hash: Some(source_hash.clone()),
                    importance: None,
                    emotional_weight: None,
                    weight: None,
                });
            }
        }
        for (idx, artifact) in derive_convo_artifacts(&content, &source_file)
            .into_iter()
            .enumerate()
        {
            if dry_run {
                continue;
            }
            let digest = format!(
                "{:x}",
                md5::compute(format!("{}:artifact:{}:{}", source_file, idx, source_hash))
            );
            let artifact_id = format!("artifact_{}_{}_{}", wing, artifact.room, &digest[..16]);
            let artifact_room = artifact.room;
            let artifact_content = artifact.content;
            let artifact_hall = artifact.hall;
            let artifact_date = artifact.date;
            let artifact_type = artifact.drawer_type;
            planned.push(DrawerInputOwned {
                id: artifact_id,
                wing: wing.clone(),
                room: artifact_room,
                source_file: source_file.clone(),
                chunk_index: (10_000 + idx) as i64,
                added_by: agent.to_string(),
                content: artifact_content,
                hall: Some(artifact_hall),
                date: artifact_date,
                drawer_type: artifact_type,
                source_hash: Some(source_hash.clone()),
                importance: artifact.importance,
                emotional_weight: None,
                weight: artifact.weight,
            });
        }

        for candidate in extract_kg_candidates(&content) {
            if candidate.confidence >= 0.8 {
                let _ = kg.add_triple(
                    &candidate.subject,
                    &candidate.predicate,
                    &candidate.object,
                    None,
                    None,
                    candidate.confidence,
                    Some(&source_file),
                    Some(&source_file),
                );
            }
        }

        if !dry_run {
            summary.drawers_added += storage.refresh_source_owned(SourceRefreshPlanOwned {
                source_file: source_file.clone(),
                source_hash: source_hash.clone(),
                drawers: planned,
            })?;
        }
    }
    Ok(summary)
}

fn scan_convos(dir: &Path, limit: Option<usize>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| should_descend(e.path()))
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && is_convo_file(path) {
            files.push(path.to_path_buf());
            if let Some(limit) = limit {
                if files.len() >= limit {
                    break;
                }
            }
        }
    }
    Ok(files)
}

fn should_descend(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        !SKIP_DIRS.contains(&name)
    } else {
        true
    }
}

fn is_convo_file(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| CONVO_EXTENSIONS.contains(&format!(".{}", ext).as_str()))
        .unwrap_or(false)
}

pub fn chunk_exchanges(content: &str) -> Vec<String> {
    let lines: Vec<_> = content.lines().collect();
    let quote_lines = lines
        .iter()
        .filter(|line| line.trim_start().starts_with('>'))
        .count();
    if quote_lines >= 3 {
        chunk_by_exchange(&lines)
    } else {
        chunk_by_paragraph(content)
    }
}

fn chunk_by_exchange(lines: &[&str]) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with('>') {
            let user_turn = line.to_string();
            i += 1;
            let mut ai_lines = Vec::new();
            while i < lines.len() {
                let next_line = lines[i].trim();
                if next_line.starts_with('>') || next_line.starts_with("---") {
                    break;
                }
                if !next_line.is_empty() {
                    ai_lines.push(next_line.to_string());
                }
                i += 1;
            }
            let ai_response = ai_lines.into_iter().take(8).collect::<Vec<_>>().join(" ");
            let chunk = if ai_response.is_empty() {
                user_turn
            } else {
                format!("{}\n{}", user_turn, ai_response)
            };
            if chunk.trim().len() > MIN_CHUNK_SIZE {
                chunks.push(chunk);
            }
        } else {
            i += 1;
        }
    }
    chunks
}

fn chunk_by_paragraph(content: &str) -> Vec<String> {
    let paragraphs: Vec<_> = content
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.len() <= 1 && content.lines().count() > 20 {
        return content
            .lines()
            .collect::<Vec<_>>()
            .chunks(25)
            .filter_map(|group| {
                let joined = group.join("\n");
                if joined.trim().len() > MIN_CHUNK_SIZE {
                    Some(joined)
                } else {
                    None
                }
            })
            .collect();
    }
    paragraphs
        .into_iter()
        .filter(|p| p.len() > MIN_CHUNK_SIZE)
        .map(ToString::to_string)
        .collect()
}

fn infer_convo_hall(room: &str, content: &str, source_file: &str, derived: bool) -> String {
    let lower = content.to_lowercase();
    let source_lower = source_file.to_lowercase();
    if source_lower.contains("diary") || source_lower.contains("journal") {
        return "hall_events".to_string();
    }
    match room {
        "decision" | "decisions" => "hall_facts".to_string(),
        "preference" | "preferences" => "hall_preferences".to_string(),
        "milestone" => "hall_events".to_string(),
        "problem" | "problems" => "hall_events".to_string(),
        "emotional" => "hall_discoveries".to_string(),
        "architecture" => "hall_facts".to_string(),
        "planning" => "hall_events".to_string(),
        "technical"
            if lower.contains("error") || lower.contains("failed") || lower.contains("fix") =>
        {
            "hall_events".to_string()
        }
        "technical"
            if lower.contains("decided")
                || lower.contains("architecture")
                || lower.contains("schema") =>
        {
            "hall_facts".to_string()
        }
        "technical" if derived => "hall_facts".to_string(),
        _ if lower.contains("prefer") || lower.contains("always") || lower.contains("never") => {
            "hall_preferences".to_string()
        }
        _ if lower.contains("decided") || lower.contains("because") || lower.contains("chose") => {
            "hall_facts".to_string()
        }
        _ if lower.contains("issue") || lower.contains("problem") || lower.contains("resolved") => {
            "hall_events".to_string()
        }
        _ if lower.contains("learned") || lower.contains("realized") => {
            "hall_discoveries".to_string()
        }
        _ => "hall_events".to_string(),
    }
}

fn detect_convo_room(content: &str) -> String {
    let keywords: &[(&str, &[&str])] = &[
        (
            "technical",
            &[
                "code", "python", "function", "bug", "error", "api", "database", "server",
                "deploy", "git", "test", "debug", "refactor",
            ],
        ),
        (
            "architecture",
            &[
                "architecture",
                "design",
                "pattern",
                "structure",
                "schema",
                "interface",
                "module",
                "component",
                "service",
                "layer",
            ],
        ),
        (
            "planning",
            &[
                "plan",
                "roadmap",
                "milestone",
                "deadline",
                "priority",
                "sprint",
                "backlog",
                "scope",
                "requirement",
                "spec",
            ],
        ),
        (
            "decisions",
            &[
                "decided",
                "chose",
                "picked",
                "switched",
                "migrated",
                "replaced",
                "trade-off",
                "alternative",
                "option",
                "approach",
            ],
        ),
        (
            "problems",
            &[
                "problem",
                "issue",
                "broken",
                "failed",
                "crash",
                "stuck",
                "workaround",
                "fix",
                "solved",
                "resolved",
            ],
        ),
    ];
    let lower = content.to_lowercase();
    let mut best = ("general".to_string(), 0usize);
    for (room, terms) in keywords {
        let score = terms.iter().filter(|term| lower.contains(**term)).count();
        if score > best.1 {
            best = ((*room).to_string(), score);
        }
    }
    best.0
}

pub fn normalize(path: &Path) -> Result<String> {
    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(content);
    }
    let lines: Vec<_> = content.lines().collect();
    if lines
        .iter()
        .filter(|line| line.trim_start().starts_with('>'))
        .count()
        >= 3
    {
        return Ok(content);
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    if ext == "json"
        || ext == "jsonl"
        || content.trim_start().starts_with('{')
        || content.trim_start().starts_with('[')
    {
        if let Some(normalized) = try_normalize_json(&content) {
            return Ok(normalized);
        }
    }
    Ok(content)
}

fn try_normalize_json(content: &str) -> Option<String> {
    if let Some(v) = try_claude_code_jsonl(content) {
        return Some(v);
    }
    let data: Value = serde_json::from_str(content).ok()?;
    try_claude_ai_json(&data)
        .or_else(|| try_chatgpt_json(&data))
        .or_else(|| try_slack_json(&data))
}

fn try_claude_code_jsonl(content: &str) -> Option<String> {
    let mut messages = Vec::new();
    for line in content.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(msg_type) = entry.get("type").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(message_content) = entry.get("message").and_then(|m| m.get("content")) else {
            continue;
        };
        let Some(content) = extract_content(message_content) else {
            continue;
        };
        match msg_type {
            "human" => messages.push(("user", content)),
            "assistant" => messages.push(("assistant", content)),
            _ => {}
        }
    }
    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_claude_ai_json(data: &Value) -> Option<String> {
    let arr = if data.is_array() {
        data.as_array()?
    } else {
        data.get("messages")?.as_array()?
    };
    let mut messages = Vec::new();
    for item in arr {
        let role = item.get("role")?.as_str()?;
        let text = extract_content(item.get("content")?)?;
        match role {
            "user" | "human" => messages.push(("user", text)),
            "assistant" | "ai" => messages.push(("assistant", text)),
            _ => {}
        }
    }
    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_chatgpt_json(data: &Value) -> Option<String> {
    let mapping = data.get("mapping")?.as_object()?;
    let mut root = None;
    let mut fallback = None;
    for (node_id, node) in mapping {
        let parent_is_null = node.get("parent").map(|v| v.is_null()).unwrap_or(false);
        if parent_is_null {
            let message_is_null = node.get("message").map(|v| v.is_null()).unwrap_or(false);
            if message_is_null {
                root = Some(node_id.clone());
                break;
            }
            if fallback.is_none() {
                fallback = Some(node_id.clone());
            }
        }
    }
    let mut current = root.or(fallback)?;
    let mut messages = Vec::new();
    let mut visited = std::collections::HashSet::new();
    while visited.insert(current.clone()) {
        let node = mapping.get(&current)?;
        if let Some(msg) = node.get("message") {
            let role = msg.get("author")?.get("role")?.as_str()?;
            let parts = msg.get("content")?.get("parts")?.as_array()?;
            let text = parts
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if !text.trim().is_empty() {
                match role {
                    "user" => messages.push(("user", text)),
                    "assistant" => messages.push(("assistant", text)),
                    _ => {}
                }
            }
        }

        let next = node
            .get("children")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        match next {
            Some(next_id) => current = next_id,
            None => break,
        }
    }
    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_slack_json(data: &Value) -> Option<String> {
    let arr = data.as_array()?;
    let mut messages = Vec::new();
    let mut seen = HashMap::new();
    let mut last_role = None::<String>;
    for item in arr {
        if item.get("type")?.as_str()? != "message" {
            continue;
        }
        let user_id = item
            .get("user")
            .or_else(|| item.get("username"))?
            .as_str()?
            .to_string();
        let text = item.get("text")?.as_str()?.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if !seen.contains_key(&user_id) {
            let role = if seen.is_empty() {
                "user"
            } else if last_role.as_deref() == Some("user") {
                "assistant"
            } else {
                "user"
            };
            seen.insert(user_id.clone(), role.to_string());
        }
        let role = seen.get(&user_id)?.clone();
        last_role = Some(role.clone());
        messages.push((role, text));
    }
    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn extract_content(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(obj)
                        if obj.get("type").and_then(|v| v.as_str()) == Some("text") =>
                    {
                        obj.get("text")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string(),
        ),
        Value::Object(obj) => obj
            .get("text")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string()),
        _ => None,
    }
}

fn messages_to_transcript(messages: &[(impl AsRef<str>, String)]) -> String {
    let mut out = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        let role = messages[i].0.as_ref();
        let text = &messages[i].1;
        if role == "user" {
            out.push(format!("> {}", text));
            if i + 1 < messages.len() && messages[i + 1].0.as_ref() == "assistant" {
                out.push(messages[i + 1].1.clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            out.push(text.clone());
            i += 1;
        }
        out.push(String::new());
    }
    out.join("\n")
}
