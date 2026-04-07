use crate::artifacts::{derive_project_artifacts, extract_kg_candidates, infer_date};
use crate::kg::KnowledgeGraph;
use crate::limits::{MAX_INGEST_CHARS, MAX_INGEST_FILE_BYTES};
use crate::search::{chunk_text, slugify};
use crate::storage::Storage;
use crate::storage_types::{DrawerInputOwned, SourceRefreshPlanOwned};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const CHUNK_SIZE: usize = 800;
const CHUNK_OVERLAP: usize = 100;
const MIN_CHUNK_SIZE: usize = 50;

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
    "coverage",
    ".mempalace",
    "target",
];

const READABLE_EXTENSIONS: &[&str] = &[
    ".txt", ".md", ".py", ".js", ".ts", ".jsx", ".tsx", ".json", ".yaml", ".yml", ".html", ".css",
    ".java", ".go", ".rs", ".rb", ".sh", ".csv", ".sql", ".toml",
];

#[derive(Debug, Deserialize)]
pub struct RoomConfig {
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    #[serde(default)]
    pub wing: Option<String>,
    #[serde(default)]
    pub rooms: Vec<RoomConfig>,
}

#[derive(Debug, Clone)]
pub struct MineSummary {
    pub wing: String,
    pub files_seen: usize,
    pub files_skipped: usize,
    pub drawers_added: usize,
    pub room_counts: HashMap<String, usize>,
}

pub fn load_project_config(project_dir: &Path) -> Result<ProjectConfig> {
    let primary = project_dir.join("mempalace.yaml");
    let legacy = project_dir.join("mempal.yaml");
    let cfg_path = if primary.exists() { primary } else { legacy };
    let raw = fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading project config {}", cfg_path.display()))?;
    let mut cfg: ProjectConfig =
        serde_yaml::from_str(&raw).with_context(|| format!("parsing {}", cfg_path.display()))?;
    if !cfg.rooms.iter().any(|r| r.name == "general") {
        cfg.rooms.push(RoomConfig {
            name: "general".to_string(),
            keywords: vec![],
        });
    }
    Ok(cfg)
}

pub fn init_project(project_dir: &Path) -> Result<PathBuf> {
    let path = project_dir.join("mempalace.yaml");
    if path.exists() {
        return Ok(path);
    }

    let wing = slugify(&infer_wing_name(project_dir, "project"));
    let mut room_names = vec!["general".to_string()];
    for entry in fs::read_dir(project_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !SKIP_DIRS.contains(&name.as_str()) {
                room_names.push(slugify(&name));
            }
        }
    }
    room_names.sort();
    room_names.dedup();
    let rooms: Vec<serde_json::Value> = room_names
        .iter()
        .map(|name| serde_json::json!({"name": name, "keywords": []}))
        .collect();
    let yaml = serde_yaml::to_string(&serde_json::json!({
        "wing": wing,
        "rooms": rooms,
    }))?;
    fs::write(&path, yaml)?;
    Ok(path)
}

pub fn mine_project(
    project_dir: &Path,
    storage: &mut Storage,
    wing_override: Option<&str>,
    agent: &str,
    limit: Option<usize>,
    dry_run: bool,
) -> Result<MineSummary> {
    let cfg = load_project_config(project_dir)?;
    let wing = wing_override
        .map(|s| s.to_string())
        .or(cfg.wing)
        .unwrap_or_else(|| slugify(&infer_wing_name(project_dir, "project")));
    let files = scan_project(project_dir, limit)?;
    let mut summary = MineSummary {
        wing: wing.clone(),
        files_seen: files.len(),
        files_skipped: 0,
        drawers_added: 0,
        room_counts: HashMap::new(),
    };

    let kg = KnowledgeGraph::open(&storage_base_dir(storage)?)?;

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
        let content = match fs::read_to_string(&file) {
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
        let room = detect_room(&file, &content, &cfg.rooms, project_dir);
        let hall = infer_project_hall(&room, &content, &source_file);
        let inferred_date = infer_date(&content, Some(&source_file));
        let chunks = chunk_text(&content, CHUNK_SIZE, CHUNK_OVERLAP, MIN_CHUNK_SIZE);
        if chunks.is_empty() {
            continue;
        }
        *summary.room_counts.entry(room.clone()).or_insert(0) += 1;
        if dry_run {
            continue;
        }
        let mut planned: Vec<DrawerInputOwned> = Vec::new();
        for (idx, chunk) in chunks.iter().enumerate() {
            let digest = format!(
                "{:x}",
                md5::compute(format!("{}:{}:{}", source_file, idx, source_hash))
            );
            let drawer_id = format!("drawer_{}_{}_{}", wing, room, &digest[..16]);
            planned.push(DrawerInputOwned {
                id: drawer_id,
                wing: wing.clone(),
                room: room.clone(),
                source_file: source_file.clone(),
                chunk_index: idx as i64,
                added_by: agent.to_string(),
                content: chunk.clone(),
                hall: Some(hall.clone()),
                date: inferred_date.clone(),
                drawer_type: "drawer".to_string(),
                source_hash: Some(source_hash.clone()),
                importance: None,
                emotional_weight: None,
                weight: None,
            });
        }

        for (idx, artifact) in derive_project_artifacts(&content, &source_file, &room)
            .into_iter()
            .enumerate()
        {
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
        summary.drawers_added += storage.refresh_source_owned(SourceRefreshPlanOwned {
            source_file: source_file.clone(),
            source_hash: source_hash.clone(),
            drawers: planned,
        })?;
    }

    Ok(summary)
}

fn scan_project(project_dir: &Path, limit: Option<usize>) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(project_dir)
        .into_iter()
        .filter_entry(|e| should_descend(e.path()))
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && is_readable(path) && !is_special_skip(path) {
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

fn is_readable(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| READABLE_EXTENSIONS.contains(&format!(".{}", ext).as_str()))
        .unwrap_or(false)
}

fn is_special_skip(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|s| s.to_str()),
        Some(
            "mempalace.yaml"
                | "mempalace.yml"
                | "mempal.yaml"
                | "mempal.yml"
                | ".gitignore"
                | "package-lock.json"
        )
    )
}

fn infer_project_hall(room: &str, content: &str, source_file: &str) -> String {
    let lower = content.to_lowercase();
    let source_lower = source_file.to_lowercase();

    if room.contains("diary") || source_lower.contains("diary") || source_lower.contains("journal")
    {
        return "hall_events".to_string();
    }
    if room.contains("decision")
        || [
            "decided",
            "decision",
            "chose",
            "switched",
            "migrated",
            "trade-off",
        ]
        .iter()
        .any(|t| lower.contains(t))
    {
        return "hall_facts".to_string();
    }
    if room.contains("preference")
        || ["prefer", "always", "never", "like", "hate", "favorite"]
            .iter()
            .any(|t| lower.contains(t))
    {
        return "hall_preferences".to_string();
    }
    if room.contains("problem")
        || [
            "fixed", "debug", "issue", "problem", "failed", "resolved", "incident",
        ]
        .iter()
        .any(|t| lower.contains(t))
    {
        return "hall_events".to_string();
    }
    if room.contains("plan")
        || room.contains("roadmap")
        || [
            "roadmap",
            "milestone",
            "deadline",
            "next step",
            "timeline",
            "plan",
        ]
        .iter()
        .any(|t| lower.contains(t))
    {
        return "hall_events".to_string();
    }
    if room.contains("advice")
        || [
            "should",
            "recommend",
            "suggest",
            "advice",
            "better",
            "avoid",
        ]
        .iter()
        .any(|t| lower.contains(t))
    {
        return "hall_advice".to_string();
    }
    if room.contains("architecture")
        || room.contains("design")
        || ["architecture", "schema", "design", "interface", "layer"]
            .iter()
            .any(|t| lower.contains(t))
    {
        return "hall_facts".to_string();
    }
    if [
        "discovered",
        "realized",
        "insight",
        "learned",
        "breakthrough",
    ]
    .iter()
    .any(|t| lower.contains(t))
    {
        return "hall_discoveries".to_string();
    }
    if room == "general" {
        "hall_events".to_string()
    } else {
        "hall_facts".to_string()
    }
}

fn storage_base_dir(_storage: &Storage) -> Result<std::path::PathBuf> {
    crate::config::default_config_dir()
}

pub(crate) fn infer_wing_name(path: &Path, fallback: &str) -> String {
    let generic = [
        "project", "projects", "repo", "repos", "src", "source", "code", "app", "apps", "convos",
        "chat", "chats",
    ];
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback);
    if !generic.contains(&file_name) {
        return file_name.to_string();
    }
    path.parent()
        .and_then(|parent| parent.file_name())
        .and_then(|s| s.to_str())
        .filter(|name| !name.is_empty() && !generic.contains(name))
        .unwrap_or(file_name)
        .to_string()
}

fn detect_room(
    filepath: &Path,
    content: &str,
    rooms: &[RoomConfig],
    project_path: &Path,
) -> String {
    let rel = filepath
        .strip_prefix(project_path)
        .unwrap_or(filepath)
        .to_string_lossy()
        .to_lowercase();
    let filename = filepath
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let content_lower = content
        .chars()
        .take(2000)
        .collect::<String>()
        .to_lowercase();

    let normalized_rel = rel.replace('\\', "/");
    let parts: Vec<_> = normalized_rel.split('/').collect();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        for room in rooms {
            let room_name = room.name.to_lowercase();
            if room_name.contains(part) || part.contains(&room_name) {
                return room.name.clone();
            }
        }
    }

    for room in rooms {
        let room_name = room.name.to_lowercase();
        if room_name.contains(&filename) || filename.contains(&room_name) {
            return room.name.clone();
        }
    }

    let mut best: Option<(String, usize)> = None;
    for room in rooms {
        let mut score = content_lower.matches(&room.name.to_lowercase()).count();
        for keyword in &room.keywords {
            score += content_lower.matches(&keyword.to_lowercase()).count();
        }
        if score > 0 && best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
            best = Some((room.name.clone(), score));
        }
    }
    best.map(|(name, _)| name)
        .unwrap_or_else(|| "general".to_string())
}
