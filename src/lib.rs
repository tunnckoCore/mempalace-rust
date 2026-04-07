pub mod artifacts;
pub mod bench;
pub mod cli;
pub mod compression;
pub mod config;
pub mod convo;
pub mod dialect;
pub mod embedding;
pub mod extractor;
pub mod graph;
pub mod kg;
pub mod layers;
pub mod limits;
pub mod mcp;
pub mod project;
pub mod search;
pub mod storage;
pub mod storage_types;
pub mod wakeup;

pub use crate::embedding::{
    embedding_preference_from_str, local_provider_from_str, runtime_config_from_sources,
    validate_runtime_config, EmbeddingPreference, EmbeddingRuntimeConfig, LocalEmbeddingProvider,
};
