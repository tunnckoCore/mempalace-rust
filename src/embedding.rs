use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use rust_stemmers::{Algorithm, Stemmer};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
#[cfg(feature = "onnx-embeddings")]
use std::sync::OnceLock;
use std::time::Duration;

pub const EMBED_DIM: usize = 512;
const MAX_NGRAMS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingBackend {
    OpenAi,
    LocalOnnx,
    LocalBuiltin,
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
    OpenAi,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalEmbeddingProvider {
    Auto,
    Builtin,
    Onnx,
}

#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRuntimeConfig {
    pub preference: EmbeddingPreference,
    pub local_provider: LocalEmbeddingProvider,
    pub openai: Option<OpenAiEmbeddingConfig>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingItem {
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingItem>,
}

pub fn embedding_preference_from_env() -> EmbeddingPreference {
    embedding_preference_from_str(
        &env::var("MEMPALACE_EMBEDDING_BACKEND").unwrap_or_else(|_| "auto".to_string()),
    )
}

pub fn embedding_preference_from_str(value: &str) -> EmbeddingPreference {
    match value.to_lowercase().as_str() {
        "openai" => EmbeddingPreference::OpenAi,
        "local" | "strong_local" | "fallback" | "onnx" | "fastembed" => EmbeddingPreference::Local,
        _ => EmbeddingPreference::Auto,
    }
}

pub fn local_provider_from_str(value: &str) -> LocalEmbeddingProvider {
    match value.to_lowercase().as_str() {
        "onnx" | "fastembed" => LocalEmbeddingProvider::Onnx,
        "builtin" | "built_in" | "strong_local" | "fallback" => LocalEmbeddingProvider::Builtin,
        _ => LocalEmbeddingProvider::Auto,
    }
}

pub fn runtime_config_from_sources(
    backend: Option<&str>,
    local_provider: Option<&str>,
    openai_model: Option<&str>,
    openai_api_key: Option<&str>,
    openai_base_url: Option<&str>,
) -> EmbeddingRuntimeConfig {
    let preference = backend
        .map(embedding_preference_from_str)
        .unwrap_or_else(embedding_preference_from_env);
    let local_provider = local_provider
        .map(local_provider_from_str)
        .or_else(|| {
            env::var("MEMPALACE_LOCAL_EMBEDDING_PROVIDER")
                .ok()
                .map(|value| local_provider_from_str(&value))
        })
        .unwrap_or(LocalEmbeddingProvider::Auto);
    let api_key = openai_api_key
        .map(ToString::to_string)
        .or_else(|| env::var("OPENAI_API_KEY").ok())
        .or_else(|| env::var("MEMPALACE_OPENAI_API_KEY").ok());
    let model = openai_model
        .map(ToString::to_string)
        .or_else(|| env::var("MEMPALACE_OPENAI_EMBEDDING_MODEL").ok())
        .unwrap_or_else(|| "text-embedding-3-small".to_string());
    let base_url = openai_base_url
        .map(ToString::to_string)
        .or_else(|| env::var("MEMPALACE_OPENAI_BASE_URL").ok())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    EmbeddingRuntimeConfig {
        preference,
        local_provider,
        openai: api_key.map(|api_key| OpenAiEmbeddingConfig {
            api_key,
            model,
            base_url,
        }),
    }
}

pub fn apply_runtime_config(config: &EmbeddingRuntimeConfig) {
    env::set_var(
        "MEMPALACE_EMBEDDING_BACKEND",
        match config.preference {
            EmbeddingPreference::Auto => "auto",
            EmbeddingPreference::OpenAi => "openai",
            EmbeddingPreference::Local => "local",
        },
    );
    env::set_var(
        "MEMPALACE_LOCAL_EMBEDDING_PROVIDER",
        match config.local_provider {
            LocalEmbeddingProvider::Auto => "auto",
            LocalEmbeddingProvider::Builtin => "builtin",
            LocalEmbeddingProvider::Onnx => "onnx",
        },
    );
    if let Some(openai) = &config.openai {
        env::set_var("OPENAI_API_KEY", &openai.api_key);
        env::set_var("MEMPALACE_OPENAI_EMBEDDING_MODEL", &openai.model);
        env::set_var("MEMPALACE_OPENAI_BASE_URL", &openai.base_url);
    }
}

pub fn validate_runtime_config(config: &EmbeddingRuntimeConfig) -> Result<()> {
    if config.preference == EmbeddingPreference::OpenAi {
        let openai = config
            .openai
            .as_ref()
            .context("OpenAI backend requested but no API key was configured")?;
        if openai.model.trim().is_empty() {
            bail!("OpenAI backend requested but model is empty");
        }
        if openai.base_url.trim().is_empty() {
            bail!("OpenAI backend requested but base URL is empty");
        }
    }
    Ok(())
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

    if preference != EmbeddingPreference::Local {
        match try_real_embedding(text, preference) {
            Ok(Some((vector, backend))) => {
                return EmbeddingResult { vector, backend };
            }
            Ok(None) => {}
            Err(err) => {
                if preference == EmbeddingPreference::OpenAi {
                    panic!("OpenAI embedding backend failed: {err}");
                }
            }
        }
    } else if let Some((vector, backend)) = try_local_provider_embedding(text) {
        return EmbeddingResult { vector, backend };
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
        backend: EmbeddingBackend::LocalBuiltin,
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

fn try_real_embedding(
    text: &str,
    preference: EmbeddingPreference,
) -> Result<Option<(Vec<f32>, EmbeddingBackend)>> {
    if preference == EmbeddingPreference::Local {
        return Ok(try_local_provider_embedding(text));
    }
    if matches!(
        preference,
        EmbeddingPreference::Auto | EmbeddingPreference::OpenAi
    ) {
        match try_openai_embedding(text) {
            Ok(Some(vector)) => return Ok(Some((vector, EmbeddingBackend::OpenAi))),
            Ok(None) => {
                if preference == EmbeddingPreference::OpenAi {
                    bail!(
                        "OpenAI embedding backend requested but OPENAI_API_KEY is not configured"
                    );
                }
            }
            Err(err) => {
                if preference == EmbeddingPreference::OpenAi {
                    return Err(err);
                }
            }
        }
    }

    if let Some(vector) = try_local_provider_embedding(text) {
        return Ok(Some(vector));
    }

    if preference == EmbeddingPreference::OpenAi {
        bail!("OpenAI embedding backend requested but no OpenAI vector could be produced");
    }

    Ok(None)
}

fn try_local_provider_embedding(text: &str) -> Option<(Vec<f32>, EmbeddingBackend)> {
    match env::var("MEMPALACE_LOCAL_EMBEDDING_PROVIDER")
        .ok()
        .as_deref()
        .map(local_provider_from_str)
        .unwrap_or(LocalEmbeddingProvider::Auto)
    {
        LocalEmbeddingProvider::Builtin => None,
        LocalEmbeddingProvider::Onnx => {
            try_onnx_embedding(text).map(|vector| (vector, EmbeddingBackend::LocalOnnx))
        }
        LocalEmbeddingProvider::Auto => {
            try_onnx_embedding(text).map(|vector| (vector, EmbeddingBackend::LocalOnnx))
        }
    }
}

#[cfg(feature = "onnx-embeddings")]
fn try_onnx_embedding(text: &str) -> Option<Vec<f32>> {
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
fn try_onnx_embedding(_text: &str) -> Option<Vec<f32>> {
    None
}

fn try_openai_embedding(text: &str) -> Result<Option<Vec<f32>>> {
    let api_key = env::var("OPENAI_API_KEY")
        .ok()
        .or_else(|| env::var("MEMPALACE_OPENAI_API_KEY").ok());
    let Some(api_key) = api_key else {
        return Ok(None);
    };
    let model = env::var("MEMPALACE_OPENAI_EMBEDDING_MODEL")
        .unwrap_or_else(|_| "text-embedding-3-small".to_string());
    let base_url = env::var("MEMPALACE_OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let endpoint = format!("{}/embeddings", base_url.trim_end_matches('/'));
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .context("building OpenAI embedding client")?;
    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&serde_json::json!({
            "input": text,
            "model": model,
        }))
        .send()
        .context("sending OpenAI embedding request")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        bail!(
            "OpenAI embedding request failed with status {}: {}",
            status,
            body
        );
    }
    let parsed: OpenAiEmbeddingResponse = response
        .json()
        .context("parsing OpenAI embedding response")?;
    let vector = parsed
        .data
        .into_iter()
        .next()
        .map(|item| pad_or_truncate(item.embedding))
        .context("OpenAI embedding response did not include data")?;
    Ok(Some(vector))
}
