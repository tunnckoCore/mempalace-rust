use crate::dialect::{CompressionStats, Dialect};
use crate::storage::{Drawer, DrawerInput, SourceRefreshPlan, Storage};
use anyhow::Result;
use md5;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CompressionArtifact {
    pub id: String,
    pub source_file: String,
    pub content: String,
    pub hall: Option<String>,
    pub date: Option<String>,
    pub importance: Option<f64>,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CompressionRunStats {
    pub total_original_chars: usize,
    pub total_compressed_chars: usize,
    pub artifacts_written: usize,
}

pub fn build_compressed_artifact(
    drawer: &Drawer,
    dialect: &Dialect,
) -> (CompressionArtifact, CompressionStats) {
    let mut meta = HashMap::new();
    meta.insert("wing".to_string(), drawer.wing.clone());
    meta.insert("room".to_string(), drawer.room.clone());
    meta.insert("source_file".to_string(), drawer.source_file.clone());
    if let Some(date) = drawer.date.clone() {
        meta.insert("date".to_string(), date);
    }
    let compressed = dialect.compress(&drawer.content, Some(&meta));
    let stats = dialect.compression_stats(&drawer.content, &compressed);
    let compressed_hash = format!("{:x}", md5::compute(compressed.as_bytes()));
    let source_file = format!("{}#aaak", drawer.source_file);
    let id = format!("aaak_{}_{}", drawer.id, &compressed_hash[..12]);
    (
        CompressionArtifact {
            id,
            source_file,
            content: compressed,
            hall: drawer.hall.clone(),
            date: drawer.date.clone(),
            importance: drawer.importance.or(Some(6.5)),
            weight: drawer.weight.or(Some(6.0)),
        },
        stats,
    )
}

pub fn maintain_compressed_artifacts(
    storage: &mut Storage,
    wing: Option<&str>,
    dry_run: bool,
) -> Result<CompressionRunStats> {
    let dialect = Dialect::new();
    let drawers = storage.sample_for_wing(wing, 10_000)?;
    let raw_drawers: Vec<_> = drawers
        .into_iter()
        .filter(|d| d.drawer_type == "drawer")
        .collect();

    let mut total_original_chars = 0usize;
    let mut total_compressed_chars = 0usize;
    let mut artifacts_written = 0usize;

    for drawer in raw_drawers {
        let (artifact, stats) = build_compressed_artifact(&drawer, &dialect);
        total_original_chars += stats.original_chars;
        total_compressed_chars += stats.compressed_chars;
        if dry_run {
            continue;
        }

        let source_hash = format!("{:x}", md5::compute(artifact.content.as_bytes()));
        if storage.source_is_current(&artifact.source_file, &source_hash)? {
            continue;
        }
        let planned = vec![DrawerInput {
            id: Box::leak(artifact.id.into_boxed_str()),
            wing: Box::leak(drawer.wing.clone().into_boxed_str()),
            room: Box::leak(drawer.room.clone().into_boxed_str()),
            source_file: Box::leak(artifact.source_file.into_boxed_str()),
            chunk_index: drawer.chunk_index,
            added_by: "compress",
            content: Box::leak(artifact.content.into_boxed_str()),
            hall: artifact.hall.as_deref(),
            date: artifact.date.as_deref(),
            drawer_type: "compressed",
            source_hash: Some(Box::leak(source_hash.clone().into_boxed_str())),
            importance: artifact.importance,
            emotional_weight: None,
            weight: artifact.weight,
        }];
        artifacts_written += storage.refresh_source(SourceRefreshPlan {
            source_file: planned[0].source_file,
            source_hash: planned[0].source_hash.unwrap_or_default(),
            drawers: planned,
        })?;
    }

    Ok(CompressionRunStats {
        total_original_chars,
        total_compressed_chars,
        artifacts_written,
    })
}

pub fn preferred_snippet(
    raw: &str,
    compressed: Option<&str>,
    prefer_compressed: bool,
    limit: usize,
) -> String {
    let chosen = if prefer_compressed {
        compressed.unwrap_or(raw)
    } else {
        raw
    };
    let flat = chosen.replace('\n', " ");
    if flat.chars().count() > limit {
        format!(
            "{}...",
            flat.chars()
                .take(limit.saturating_sub(3))
                .collect::<String>()
        )
    } else {
        flat
    }
}
