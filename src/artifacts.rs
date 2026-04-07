use crate::dialect::Dialect;
use crate::extractor::extract_memories;
use regex::Regex;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct DerivedArtifact {
    pub room: String,
    pub hall: String,
    pub drawer_type: String,
    pub content: String,
    pub date: Option<String>,
    pub importance: Option<f64>,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct KgCandidate {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f64,
}

pub fn derive_project_artifacts(
    content: &str,
    source_file: &str,
    room: &str,
) -> Vec<DerivedArtifact> {
    let inferred_date = infer_date(content, Some(source_file));
    let mut artifacts = Vec::new();
    artifacts.extend(anchor_artifacts(content, inferred_date.as_deref(), room));
    artifacts.extend(bridge_artifacts(content, inferred_date.as_deref(), room));
    artifacts.extend(aaak_artifact(
        content,
        source_file,
        room,
        inferred_date.as_deref(),
    ));
    dedupe_artifacts(artifacts)
}

pub fn derive_convo_artifacts(content: &str, source_file: &str) -> Vec<DerivedArtifact> {
    let inferred_date = infer_date(content, Some(source_file));
    let mut artifacts = Vec::new();
    artifacts.extend(anchor_artifacts(
        content,
        inferred_date.as_deref(),
        "conversation",
    ));
    artifacts.extend(bridge_artifacts(
        content,
        inferred_date.as_deref(),
        "conversation",
    ));
    artifacts.extend(aaak_artifact(
        content,
        source_file,
        "conversation",
        inferred_date.as_deref(),
    ));
    for memory in extract_memories(content, 0.3) {
        artifacts.push(DerivedArtifact {
            room: memory.memory_type.clone(),
            hall: infer_artifact_hall(
                &memory.memory_type,
                "derived_memory",
                &memory.content,
                "conversation",
            ),
            drawer_type: "derived_memory".to_string(),
            content: memory.content,
            date: inferred_date.clone(),
            importance: Some(6.0),
            weight: Some(6.0),
        });
    }
    dedupe_artifacts(artifacts)
}

pub fn extract_kg_candidates(content: &str) -> Vec<KgCandidate> {
    let mut candidates = Vec::new();
    let prefers =
        Regex::new(r"\b([A-Z][a-zA-Z0-9_]+)\s+prefers\s+([A-Z][a-zA-Z0-9_]+|[a-z][a-zA-Z0-9_-]+)")
            .unwrap();
    let uses =
        Regex::new(r"\b([A-Z][a-zA-Z0-9_]+)\s+uses\s+([A-Z][a-zA-Z0-9_]+|[a-z][a-zA-Z0-9_-]+)")
            .unwrap();
    let works_on = Regex::new(r"\b([A-Z][a-zA-Z0-9_]+)\s+works on\s+([A-Z][a-zA-Z0-9_]+)").unwrap();
    let decided = Regex::new(
        r"\b([A-Z][a-zA-Z0-9_]+)?\s*decided to use\s+([A-Z][a-zA-Z0-9_]+|[a-z][a-zA-Z0-9_-]+)",
    )
    .unwrap();

    for caps in prefers.captures_iter(content) {
        candidates.push(KgCandidate {
            subject: caps[1].to_string(),
            predicate: "prefers".to_string(),
            object: caps[2].to_string(),
            confidence: 0.85,
        });
    }
    for caps in uses.captures_iter(content) {
        candidates.push(KgCandidate {
            subject: caps[1].to_string(),
            predicate: "uses".to_string(),
            object: caps[2].to_string(),
            confidence: 0.8,
        });
    }
    for caps in works_on.captures_iter(content) {
        candidates.push(KgCandidate {
            subject: caps[1].to_string(),
            predicate: "works_on".to_string(),
            object: caps[2].to_string(),
            confidence: 0.8,
        });
    }
    for caps in decided.captures_iter(content) {
        let subject = caps.get(1).map(|m| m.as_str()).unwrap_or("team");
        candidates.push(KgCandidate {
            subject: subject.to_string(),
            predicate: "decided_to_use".to_string(),
            object: caps[2].to_string(),
            confidence: 0.75,
        });
    }

    dedupe_candidates(candidates)
}

pub fn infer_date(content: &str, source_file: Option<&str>) -> Option<String> {
    let patterns = [
        Regex::new(r"\b(20\d{2}-\d{2}-\d{2})\b").unwrap(),
        Regex::new(r"\b(20\d{2}/\d{2}/\d{2})\b").unwrap(),
        Regex::new(r"\b(20\d{2}\.\d{2}\.\d{2})\b").unwrap(),
        Regex::new(r"\b(20\d{2}-\d{2})\b").unwrap(),
        Regex::new(r"\b(20\d{2}_\d{2}_\d{2})\b").unwrap(),
    ];

    for re in patterns {
        if let Some(caps) = re.captures(content) {
            return normalize_date(caps.get(1)?.as_str());
        }
    }

    if let Some(path) = source_file {
        let filename = path.rsplit('/').next().unwrap_or(path);
        for re in [
            Regex::new(r"(20\d{2}-\d{2}-\d{2})").unwrap(),
            Regex::new(r"(20\d{2}_\d{2}_\d{2})").unwrap(),
            Regex::new(r"(20\d{2}-\d{2})").unwrap(),
        ] {
            if let Some(caps) = re.captures(filename) {
                return normalize_date(caps.get(1)?.as_str());
            }
        }
    }

    None
}

pub fn infer_artifact_hall(
    room: &str,
    drawer_type: &str,
    content: &str,
    context_room: &str,
) -> String {
    let lower = content.to_lowercase();
    if matches!(drawer_type, "preference_bridge") || room.contains("preference") {
        return "hall_preferences".to_string();
    }
    if matches!(drawer_type, "decision_bridge") || room.contains("decision") {
        return "hall_facts".to_string();
    }
    if matches!(drawer_type, "anchor_doc") {
        if context_room.contains("architecture") || context_room.contains("design") {
            return "hall_facts".to_string();
        }
        if lower.contains("issue") || lower.contains("fix") {
            return "hall_events".to_string();
        }
        return "hall_facts".to_string();
    }
    if lower.contains("learned") || lower.contains("realized") || lower.contains("insight") {
        return "hall_discoveries".to_string();
    }
    if lower.contains("problem") || lower.contains("failed") || lower.contains("resolved") {
        return "hall_events".to_string();
    }
    if context_room.contains("planning") || context_room.contains("timeline") {
        return "hall_events".to_string();
    }
    "hall_facts".to_string()
}

fn anchor_artifacts(content: &str, date: Option<&str>, context_room: &str) -> Vec<DerivedArtifact> {
    let mut artifacts = Vec::new();
    let tokens = extract_anchor_terms(content);
    for token in tokens {
        let artifact_content = format!("ANCHOR {}\n{}", token, summarize_for_anchor(content));
        artifacts.push(DerivedArtifact {
            room: "anchors".to_string(),
            hall: infer_artifact_hall("anchors", "anchor_doc", &artifact_content, context_room),
            drawer_type: "anchor_doc".to_string(),
            content: artifact_content,
            date: date.map(ToString::to_string),
            importance: Some(5.0),
            weight: Some(5.0),
        });
    }
    artifacts
}

fn aaak_artifact(
    content: &str,
    source_file: &str,
    context_room: &str,
    date: Option<&str>,
) -> Vec<DerivedArtifact> {
    let mut meta = HashMap::new();
    meta.insert("wing".to_string(), "?".to_string());
    meta.insert("room".to_string(), context_room.to_string());
    meta.insert("source_file".to_string(), source_file.to_string());
    if let Some(date) = date {
        meta.insert("date".to_string(), date.to_string());
    }
    let compressed = Dialect::new().compress(content, Some(&meta));
    vec![DerivedArtifact {
        room: context_room.to_string(),
        hall: infer_artifact_hall(context_room, "compressed", &compressed, context_room),
        drawer_type: "compressed".to_string(),
        content: compressed,
        date: date.map(ToString::to_string),
        importance: Some(7.5),
        weight: Some(7.0),
    }]
}

fn bridge_artifacts(content: &str, date: Option<&str>, context_room: &str) -> Vec<DerivedArtifact> {
    let mut artifacts = Vec::new();
    let lower = content.to_lowercase();
    if lower.contains("prefer") || lower.contains("always") || lower.contains("never") {
        let artifact_content = format!("PREFERENCE BRIDGE\n{}", summarize_for_anchor(content));
        artifacts.push(DerivedArtifact {
            room: "preferences".to_string(),
            hall: infer_artifact_hall(
                "preferences",
                "preference_bridge",
                &artifact_content,
                context_room,
            ),
            drawer_type: "preference_bridge".to_string(),
            content: artifact_content,
            date: date.map(ToString::to_string),
            importance: Some(7.0),
            weight: Some(7.0),
        });
    }
    if lower.contains("decided")
        || lower.contains("chose")
        || lower.contains("switched")
        || lower.contains("because")
    {
        let artifact_content = format!("DECISION BRIDGE\n{}", summarize_for_anchor(content));
        artifacts.push(DerivedArtifact {
            room: "decisions".to_string(),
            hall: infer_artifact_hall(
                "decisions",
                "decision_bridge",
                &artifact_content,
                context_room,
            ),
            drawer_type: "decision_bridge".to_string(),
            content: artifact_content,
            date: date.map(ToString::to_string),
            importance: Some(8.0),
            weight: Some(8.0),
        });
    }
    artifacts
}

fn extract_anchor_terms(content: &str) -> Vec<String> {
    let re = Regex::new(r"\b([A-Z][a-zA-Z0-9_]{2,}|[a-z]+(?:-[a-z0-9]+){1,}|[A-Z][a-zA-Z0-9]+(?:[A-Z][a-zA-Z0-9]+)+)\b").unwrap();
    let mut terms = BTreeSet::new();
    for cap in re.captures_iter(content) {
        let token = cap[1].to_string();
        if token.len() >= 3 {
            terms.insert(token);
        }
    }
    terms.into_iter().take(10).collect()
}

fn summarize_for_anchor(content: &str) -> String {
    let flat = content.replace('\n', " ");
    if flat.chars().count() > 180 {
        format!("{}...", flat.chars().take(177).collect::<String>())
    } else {
        flat
    }
}

fn dedupe_artifacts(artifacts: Vec<DerivedArtifact>) -> Vec<DerivedArtifact> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for artifact in artifacts {
        let key = format!(
            "{}::{}::{}",
            artifact.drawer_type, artifact.room, artifact.content
        );
        if seen.insert(key) {
            out.push(artifact);
        }
    }
    out
}

fn normalize_date(raw: &str) -> Option<String> {
    let normalized = raw.replace(['/', '_', '.'], "-");
    let parts: Vec<_> = normalized.split('-').collect();
    match parts.as_slice() {
        [year, month, day] if year.len() == 4 && month.len() == 2 && day.len() == 2 => {
            Some(format!("{}-{}-{}", year, month, day))
        }
        [year, month] if year.len() == 4 && month.len() == 2 => Some(format!("{}-{}", year, month)),
        _ => None,
    }
}

fn dedupe_candidates(candidates: Vec<KgCandidate>) -> Vec<KgCandidate> {
    let mut best: HashMap<(String, String, String), KgCandidate> = HashMap::new();
    for candidate in candidates {
        let key = (
            candidate.subject.clone(),
            candidate.predicate.clone(),
            candidate.object.clone(),
        );
        best.entry(key)
            .and_modify(|existing| {
                if candidate.confidence > existing.confidence {
                    *existing = candidate.clone();
                }
            })
            .or_insert(candidate);
    }
    best.into_values().collect()
}
