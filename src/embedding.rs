use rust_stemmers::{Algorithm, Stemmer};
use std::collections::{HashMap, HashSet};
use std::env;
#[cfg(feature = "onnx-embeddings")]
use std::sync::OnceLock;

pub const EMBED_DIM: usize = 512;
const MAX_NGRAMS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    OnnxLocal,
    StrongLocal,
    LexicalFallback,
}

#[derive(Debug, Clone)]
pub struct EmbeddingResult {
    pub vector: Vec<f32>,
    pub backend: EmbeddingBackend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingPreference {
    Auto,
    StrongLocal,
    Onnx,
}

pub fn embedding_preference_from_env() -> EmbeddingPreference {
    embedding_preference_from_str(
        &env::var("MEMPALACE_EMBEDDING_BACKEND").unwrap_or_else(|_| "auto".to_string()),
    )
}

pub fn embedding_preference_from_str(value: &str) -> EmbeddingPreference {
    match value.to_lowercase().as_str() {
        "onnx" | "fastembed" => EmbeddingPreference::Onnx,
        "local" | "strong_local" | "fallback" | "openai" => EmbeddingPreference::StrongLocal,
        _ => EmbeddingPreference::Auto,
    }
}

pub fn embed_text(text: &str) -> EmbeddingResult {
    embed_text_with_preference(text, embedding_preference_from_env())
}

pub fn embed_text_with_preference(text: &str, preference: EmbeddingPreference) -> EmbeddingResult {
    let terms = normalized_terms(text);
    if terms.is_empty() {
        return EmbeddingResult {
            vector: vec![0.0; EMBED_DIM],
            backend: EmbeddingBackend::LexicalFallback,
        };
    }

    if preference != EmbeddingPreference::StrongLocal {
        if let Some(vector) = try_real_embedding(text, preference) {
            return EmbeddingResult {
                vector,
                backend: EmbeddingBackend::OnnxLocal,
            };
        }
    }

    let mut weights: HashMap<String, f32> = HashMap::new();
    for term in &terms {
        *weights.entry(term.clone()).or_insert(0.0) += 1.0;
    }

    for window in 2..=MAX_NGRAMS {
        for ngram in term_ngrams(&terms, window) {
            *weights.entry(ngram).or_insert(0.0) += 1.75 + (window as f32 - 2.0) * 0.5;
        }
    }

    let mut vector = vec![0.0_f32; EMBED_DIM];
    for (term, weight) in &weights {
        let (primary, secondary, sign) = feature_positions(term);
        vector[primary] += *weight;
        vector[secondary] += *weight * 0.5 * sign;
    }

    normalize(&mut vector);
    EmbeddingResult {
        vector,
        backend: EmbeddingBackend::StrongLocal,
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;
    for (left, right) in a.iter().zip(b.iter()) {
        dot += left * right;
        norm_a += left * left;
        norm_b += right * right;
    }
    if norm_a <= f32::EPSILON || norm_b <= f32::EPSILON {
        0.0
    } else {
        dot / (norm_a.sqrt() * norm_b.sqrt())
    }
}

pub fn phrase_overlap_score(query: &str, text: &str) -> f32 {
    let query_lower = query.to_lowercase();
    let text_lower = text.to_lowercase();
    if query_lower.trim().is_empty() || text_lower.trim().is_empty() {
        return 0.0;
    }

    let mut score = 0.0_f32;
    if text_lower.contains(query_lower.trim()) {
        score += 1.0;
    }

    let q_terms = normalized_terms(query);
    let d_terms = normalized_terms(text);
    if q_terms.is_empty() || d_terms.is_empty() {
        return score;
    }

    let q_set: HashSet<_> = q_terms.iter().cloned().collect();
    let d_set: HashSet<_> = d_terms.iter().cloned().collect();
    let overlap = q_set.intersection(&d_set).count() as f32 / q_set.len().max(1) as f32;
    score + overlap * 0.75
}

pub fn named_entityish_boost(query: &str, text: &str) -> f32 {
    let names = query
        .split_whitespace()
        .filter(|token| {
            let mut chars = token.chars();
            matches!(chars.next(), Some(c) if c.is_uppercase()) && token.len() > 2
        })
        .collect::<Vec<_>>();

    if names.is_empty() {
        return 0.0;
    }

    let text_lower = text.to_lowercase();
    let hits = names
        .iter()
        .filter(|name| text_lower.contains(&name.to_lowercase()))
        .count();
    hits as f32 / names.len().max(1) as f32
}

pub fn encode_vector(vec: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vec.len() * 4);
    for value in vec {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

pub fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub fn normalized_terms(text: &str) -> Vec<String> {
    let stemmer = Stemmer::create(Algorithm::English);
    let mut terms = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            push_term(&stemmer, &mut terms, &current);
            current.clear();
        }
    }
    if !current.is_empty() {
        push_term(&stemmer, &mut terms, &current);
    }

    terms
}

fn push_term(stemmer: &Stemmer, terms: &mut Vec<String>, raw: &str) {
    if raw.len() < 2 {
        return;
    }
    let stemmed = stemmer.stem(raw).to_string();
    if stemmed.len() >= 2 {
        terms.push(stemmed);
    }
}

fn term_ngrams(terms: &[String], size: usize) -> Vec<String> {
    if terms.len() < size {
        return Vec::new();
    }
    terms
        .windows(size)
        .map(|window| window.join("::"))
        .collect()
}

fn feature_positions(term: &str) -> (usize, usize, f32) {
    let bytes = term.as_bytes();
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x9e3779b97f4a7c15;
    for b in bytes {
        h1 ^= u64::from(*b);
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= u64::from(*b).wrapping_mul(0x9e3779b1);
        h2 = h2.rotate_left(7).wrapping_mul(0xc2b2ae35);
    }
    let primary = (h1 as usize) % EMBED_DIM;
    let secondary = (h2 as usize) % EMBED_DIM;
    let sign = if (h1 ^ h2) & 1 == 0 { 1.0 } else { -1.0 };
    (primary, secondary, sign)
}

fn normalize(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vec {
            *value /= norm;
        }
    }
}

#[cfg(feature = "onnx-embeddings")]
fn pad_or_truncate(mut vec: Vec<f32>) -> Vec<f32> {
    if vec.len() > EMBED_DIM {
        vec.truncate(EMBED_DIM);
        normalize(&mut vec);
        return vec;
    }
    if vec.len() < EMBED_DIM {
        vec.resize(EMBED_DIM, 0.0);
    }
    normalize(&mut vec);
    vec
}

#[cfg(feature = "onnx-embeddings")]
fn try_real_embedding(text: &str, preference: EmbeddingPreference) -> Option<Vec<f32>> {
    if preference == EmbeddingPreference::StrongLocal {
        return None;
    }
    static MODEL: OnceLock<Option<fastembed::TextEmbedding>> = OnceLock::new();
    let model = MODEL.get_or_init(|| {
        let cache_dir = env::var("MEMPALACE_EMBEDDING_MODEL_DIR").ok();
        let mut builder = fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2);
        if let Some(dir) = cache_dir {
            builder = builder.with_cache_dir(dir.into());
        }
        fastembed::TextEmbedding::try_new(builder).ok()
    });
    let model = model.as_ref()?;
    let vectors = model.embed(vec![text.to_string()], None).ok()?;
    vectors.into_iter().next().map(pad_or_truncate)
}

#[cfg(not(feature = "onnx-embeddings"))]
fn try_real_embedding(_text: &str, _preference: EmbeddingPreference) -> Option<Vec<f32>> {
    None
}
