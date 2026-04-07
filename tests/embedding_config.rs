use mempalace_rust::embedding::{
    embedding_preference_from_str, runtime_config_from_sources, validate_runtime_config,
    EmbeddingPreference,
};

#[test]
fn runtime_config_prefers_cli_over_env() {
    std::env::set_var("MEMPALACE_EMBEDDING_BACKEND", "strong_local");
    std::env::set_var("OPENAI_API_KEY", "env-key");
    let config = runtime_config_from_sources(
        Some("openai"),
        Some("cli-model"),
        Some("cli-key"),
        Some("https://example.invalid/v1"),
    );
    assert_eq!(config.preference, EmbeddingPreference::OpenAi);
    let openai = config.openai.expect("openai config");
    assert_eq!(openai.api_key, "cli-key");
    assert_eq!(openai.model, "cli-model");
    assert_eq!(openai.base_url, "https://example.invalid/v1");
    std::env::remove_var("MEMPALACE_EMBEDDING_BACKEND");
    std::env::remove_var("OPENAI_API_KEY");
}

#[test]
fn explicit_openai_requires_key() {
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("MEMPALACE_OPENAI_API_KEY");
    let config =
        runtime_config_from_sources(Some("openai"), Some("text-embedding-3-small"), None, None);
    let err = validate_runtime_config(&config).expect_err("missing key should fail");
    assert!(err.to_string().contains("API key"));
}

#[test]
fn embedding_preference_parses_openai() {
    assert_eq!(
        embedding_preference_from_str("openai"),
        EmbeddingPreference::OpenAi
    );
}
