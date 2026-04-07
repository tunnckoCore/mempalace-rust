use crate::dialect::{CompressionStats, Dialect};
use crate::storage::{Drawer, Storage};
use crate::storage_types::{DrawerInputOwned, SourceRefreshPlanOwned};
use anyhow::Result;
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
        let planned = vec![DrawerInputOwned {
            id: artifact.id,
            wing: drawer.wing.clone(),
            room: drawer.room.clone(),
            source_file: artifact.source_file.clone(),
            chunk_index: drawer.chunk_index,
            added_by: "compress".to_string(),
            content: artifact.content,
            hall: artifact.hall,
            date: artifact.date,
            drawer_type: "compressed".to_string(),
            source_hash: Some(source_hash.clone()),
            importance: artifact.importance,
            emotional_weight: None,
            weight: artifact.weight,
        }];
        artifacts_written += storage.refresh_source_owned(SourceRefreshPlanOwned {
            source_file: artifact.source_file,
            source_hash,
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
