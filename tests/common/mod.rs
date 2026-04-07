use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::{tempdir, TempDir};

#[allow(dead_code)]
pub fn temp_palace() -> (TempDir, PathBuf) {
    let dir = tempdir().expect("tempdir");
    let palace = dir.path().join("palace");
    (dir, palace)
}

#[allow(dead_code)]
pub fn app_config(config_root: &Path, palace_path: &Path) -> mempalace_rust::config::AppConfig {
    let identity_path = config_root.join("identity.txt");
    if !identity_path.exists() {
        fs::create_dir_all(config_root).expect("create cfg dir");
        fs::write(&identity_path, "I am Atlas, memory-aware.").expect("write identity");
    }
    mempalace_rust::config::AppConfig {
        config_dir: config_root.to_path_buf(),
        config_file: config_root.join("config.json"),
        identity_file: identity_path,
        palace_path: palace_path.to_path_buf(),
        collection_name: "mempalace_drawers".to_string(),
        people_map: HashMap::new(),
        embedding_backend: "strong_local".to_string(),
        openai_embedding_model: "text-embedding-3-small".to_string(),
        openai_base_url: "https://api.openai.com/v1".to_string(),
    }
}
