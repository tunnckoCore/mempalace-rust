use crate::embedding::EmbeddingPreference;
#[cfg(test)]
use crate::embedding::{embed_text_with_preference, embedding_preference_from_str};
use crate::limits::{MAX_BENCHMARK_BYTES, MAX_BENCHMARK_DOCS};
use crate::storage::Storage;
use crate::storage_types::{DrawerInputOwned, SourceRefreshPlanOwned};
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum BenchmarkBackend {
    Fts,
    Local,
    Onnx,
    Openai,
    Hybrid,
}

#[derive(Debug, Deserialize)]
pub struct BenchmarkCase {
    #[allow(dead_code)]
    pub id: String,
    pub query: String,
    pub relevant_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct BenchmarkDoc {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub content: String,
    #[serde(default)]
    pub source_file: String,
}

#[derive(Debug, Deserialize)]
pub struct BenchmarkDataset {
    pub documents: Vec<BenchmarkDoc>,
    pub queries: Vec<BenchmarkCase>,
}

#[derive(Debug, Serialize)]
pub struct BenchmarkResult {
    pub queries: usize,
    pub recall_at_k: f64,
    pub mrr: f64,
    pub ndcg_at_k: f64,
    pub backend: String,
}

pub fn load_dataset(path: &Path) -> Result<BenchmarkDataset> {
    let metadata = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if metadata.len() > MAX_BENCHMARK_BYTES {
        anyhow::bail!(
            "benchmark dataset exceeds max size of {} bytes",
            MAX_BENCHMARK_BYTES
        );
    }
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
        let mut documents = Vec::new();
        let mut queries = Vec::new();
        for line in raw.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line)?;
            match value
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
            {
                "document" => documents.push(serde_json::from_value(value)?),
                "query" => queries.push(serde_json::from_value(value)?),
                _ => {}
            }
        }
        return Ok(BenchmarkDataset { documents, queries });
    }
    let dataset: BenchmarkDataset =
        serde_json::from_str(&raw).context("parsing benchmark dataset")?;
    if dataset.documents.len() > MAX_BENCHMARK_DOCS {
        anyhow::bail!("benchmark document count exceeds {}", MAX_BENCHMARK_DOCS);
    }
    Ok(dataset)
}

pub fn run_benchmark(
    storage: &mut Storage,
    dataset: &BenchmarkDataset,
    backend: BenchmarkBackend,
    k: usize,
) -> Result<BenchmarkResult> {
    index_dataset(storage, dataset)?;
    let mut recall_hits = 0usize;
    let mut reciprocal_sum = 0.0_f64;
    let mut ndcg_sum = 0.0_f64;

    for case in &dataset.queries {
        let results = match backend {
            BenchmarkBackend::Fts => storage.lexical_debug_search(&case.query, k)?,
            BenchmarkBackend::Hybrid => storage
                .search(&case.query, None, None, k)?
                .into_iter()
                .map(|hit| hit.id)
                .collect(),
            BenchmarkBackend::Local | BenchmarkBackend::Onnx | BenchmarkBackend::Openai => {
                let preference = match backend {
                    BenchmarkBackend::Local | BenchmarkBackend::Openai => {
                        EmbeddingPreference::StrongLocal
                    }
                    BenchmarkBackend::Onnx => EmbeddingPreference::Onnx,
                    _ => EmbeddingPreference::Auto,
                };
                storage.semantic_debug_search(&case.query, preference, k)?
            }
        };

        if let Some(rank) = first_relevant_rank(&results, &case.relevant_ids) {
            reciprocal_sum += 1.0 / rank as f64;
            if rank <= k {
                recall_hits += 1;
            }
        }
        ndcg_sum += ndcg_at_k(&results, &case.relevant_ids, k);
    }

    let queries = dataset.queries.len().max(1);
    Ok(BenchmarkResult {
        queries: dataset.queries.len(),
        recall_at_k: recall_hits as f64 / queries as f64,
        mrr: reciprocal_sum / queries as f64,
        ndcg_at_k: ndcg_sum / queries as f64,
        backend: format!("{:?}", backend).to_lowercase(),
    })
}

fn index_dataset(storage: &mut Storage, dataset: &BenchmarkDataset) -> Result<()> {
    let mut by_source: HashMap<String, Vec<&BenchmarkDoc>> = HashMap::new();
    for doc in &dataset.documents {
        let source = if doc.source_file.is_empty() {
            format!("bench/{}.md", doc.id)
        } else {
            doc.source_file.clone()
        };
        by_source.entry(source).or_default().push(doc);
    }

    for (source_file, docs) in by_source {
        let source_hash = format!(
            "{:x}",
            md5::compute(
                docs.iter()
                    .map(|doc| format!("{}:{}:{}:{}", doc.id, doc.wing, doc.room, doc.content))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        );
        let mut planned: Vec<DrawerInputOwned> = Vec::new();
        for (idx, doc) in docs.into_iter().enumerate() {
            planned.push(DrawerInputOwned {
                id: doc.id.clone(),
                wing: doc.wing.clone(),
                room: doc.room.clone(),
                source_file: source_file.clone(),
                chunk_index: idx as i64,
                added_by: "benchmark".to_string(),
                content: doc.content.clone(),
                hall: Some("hall_facts".to_string()),
                date: None,
                drawer_type: "drawer".to_string(),
                source_hash: Some(source_hash.clone()),
                importance: None,
                emotional_weight: None,
                weight: None,
            });
        }
        storage.refresh_source_owned(SourceRefreshPlanOwned {
            source_file,
            source_hash,
            drawers: planned,
        })?;
    }
    Ok(())
}

fn first_relevant_rank<T: AsRef<str>>(results: &[T], relevant_ids: &[String]) -> Option<usize> {
    results
        .iter()
        .position(|id| relevant_ids.iter().any(|rel| rel == id.as_ref()))
        .map(|idx| idx + 1)
}

fn ndcg_at_k<T: AsRef<str>>(results: &[T], relevant_ids: &[String], k: usize) -> f64 {
    let dcg = results
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, id)| relevant_ids.iter().any(|rel| rel == id.as_ref()))
        .map(|(idx, _)| 1.0 / ((idx + 2) as f64).log2())
        .sum::<f64>();
    let ideal_hits = relevant_ids.len().min(k);
    let idcg = (0..ideal_hits)
        .map(|idx| 1.0 / ((idx + 2) as f64).log2())
        .sum::<f64>();
    if idcg <= f64::EPSILON {
        0.0
    } else {
        dcg / idcg
    }
}

pub fn backend_help() -> &'static str {
    "Benchmark dataset JSON format: {\"documents\":[{\"id\",\"wing\",\"room\",\"content\",\"source_file\"?}],\"queries\":[{\"id\",\"query\",\"relevant_ids\":[...] }]}. JSONL format supported with kind=document/query. Output includes recall_at_k, mrr, ndcg_at_k."
}

#[cfg(test)]
pub fn preview_embedding_backend(name: &str, text: &str) -> String {
    let preference = embedding_preference_from_str(name);
    let result = embed_text_with_preference(text, preference);
    format!("{:?}", result.backend).to_lowercase()
}

#[cfg(test)]
pub fn sort_ids_by_score(mut ids: Vec<(String, f64)>, k: usize) -> Vec<String> {
    ids.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
    ids.into_iter().take(k).map(|(id, _)| id).collect()
}
