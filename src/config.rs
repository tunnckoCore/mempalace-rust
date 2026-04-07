use anyhow::{Context, Result};
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_COLLECTION_NAME: &str = "mempalace_drawers";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub palace_path: Option<String>,
    #[serde(default)]
    pub collection_name: Option<String>,
    #[serde(default)]
    pub people_map: HashMap<String, String>,
    #[serde(default)]
    pub embedding_backend: Option<String>,
    #[serde(default)]
    pub local_embedding_provider: Option<String>,
    #[serde(default)]
    pub openai_embedding_model: Option<String>,
    #[serde(default)]
    pub openai_base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub identity_file: PathBuf,
    pub palace_path: PathBuf,
    pub collection_name: String,
    pub people_map: HashMap<String, String>,
    pub embedding_backend: String,
    pub local_embedding_provider: String,
    pub openai_embedding_model: String,
    pub openai_base_url: String,
}

impl AppConfig {
    pub fn load(override_palace: Option<&Path>) -> Result<Self> {
        let config_dir = default_config_dir()?;
        let config_file = config_dir.join("config.json");
        let identity_file = config_dir.join("identity.txt");

        let file_cfg = if config_file.exists() {
            let raw = fs::read_to_string(&config_file)
                .with_context(|| format!("reading {}", config_file.display()))?;
            serde_json::from_str::<FileConfig>(&raw).unwrap_or(FileConfig {
                palace_path: None,
                collection_name: None,
                people_map: HashMap::new(),
                embedding_backend: None,
                local_embedding_provider: None,
                openai_embedding_model: None,
                openai_base_url: None,
            })
        } else {
            FileConfig {
                palace_path: None,
                collection_name: None,
                people_map: HashMap::new(),
                embedding_backend: None,
                local_embedding_provider: None,
                openai_embedding_model: None,
                openai_base_url: None,
            }
        };

        let palace_path = if let Some(path) = override_palace {
            path.to_path_buf()
        } else if let Some(val) = env::var_os("MEMPALACE_PALACE_PATH") {
            PathBuf::from(val)
        } else if let Some(val) = env::var_os("MEMPAL_PALACE_PATH") {
            PathBuf::from(val)
        } else if let Some(val) = file_cfg.palace_path.as_deref() {
            expand_tilde(val)
        } else {
            default_palace_path()?
        };

        Ok(Self {
            config_dir,
            config_file,
            identity_file,
            palace_path,
            collection_name: file_cfg
                .collection_name
                .unwrap_or_else(|| DEFAULT_COLLECTION_NAME.to_string()),
            people_map: file_cfg.people_map,
            embedding_backend: env::var("MEMPALACE_EMBEDDING_BACKEND")
                .ok()
                .or(file_cfg.embedding_backend)
                .unwrap_or_else(|| "auto".to_string()),
            local_embedding_provider: env::var("MEMPALACE_LOCAL_EMBEDDING_PROVIDER")
                .ok()
                .or(file_cfg.local_embedding_provider)
                .unwrap_or_else(|| "auto".to_string()),
            openai_embedding_model: env::var("MEMPALACE_OPENAI_EMBEDDING_MODEL")
                .ok()
                .or(file_cfg.openai_embedding_model)
                .unwrap_or_else(|| "text-embedding-3-small".to_string()),
            openai_base_url: env::var("MEMPALACE_OPENAI_BASE_URL")
                .ok()
                .or(file_cfg.openai_base_url)
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        })
    }

    pub fn init_files(&self) -> Result<()> {
        fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("creating {}", self.config_dir.display()))?;
        if !self.config_file.exists() {
            let body = serde_json::to_string_pretty(&FileConfig {
                palace_path: Some(self.palace_path.to_string_lossy().to_string()),
                collection_name: Some(self.collection_name.clone()),
                people_map: self.people_map.clone(),
                embedding_backend: Some(self.embedding_backend.clone()),
                local_embedding_provider: Some(self.local_embedding_provider.clone()),
                openai_embedding_model: Some(self.openai_embedding_model.clone()),
                openai_base_url: Some(self.openai_base_url.clone()),
            })?;
            fs::write(&self.config_file, body)
                .with_context(|| format!("writing {}", self.config_file.display()))?;
        }
        Ok(())
    }
}

pub fn default_config_dir() -> Result<PathBuf> {
    Ok(home_dir()
        .context("could not determine home directory")?
        .join(".mempalace"))
}

pub fn default_palace_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join("palace"))
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if input == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/") {
        return home_dir()
            .map(|home| home.join(rest))
            .unwrap_or_else(|| PathBuf::from(input));
    }
    PathBuf::from(input)
}
