use crate::compression::preferred_snippet;
use crate::config::AppConfig;
use crate::storage::{Drawer, Storage};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

pub struct Layer0 {
    path: std::path::PathBuf,
}

impl Layer0 {
    pub fn new(identity_path: &Path) -> Self {
        Self {
            path: identity_path.to_path_buf(),
        }
    }

    pub fn render(&self) -> String {
        if self.path.exists() {
            std::fs::read_to_string(&self.path)
                .unwrap_or_default()
                .trim()
                .to_string()
        } else {
            "## L0 — IDENTITY\nNo identity configured. Create ~/.mempalace/identity.txt".to_string()
        }
    }

    pub fn token_estimate(&self) -> usize {
        self.render().len() / 4
    }
}

pub struct Layer1<'a> {
    storage: &'a Storage,
    wing: Option<String>,
    prefer_compressed: bool,
}

impl<'a> Layer1<'a> {
    const MAX_DRAWERS: usize = 15;
    const MAX_CHARS: usize = 3200;

    pub fn new(storage: &'a Storage, wing: Option<&str>, prefer_compressed: bool) -> Self {
        Self {
            storage,
            wing: wing.map(ToString::to_string),
            prefer_compressed,
        }
    }

    pub fn generate(&self) -> Result<String> {
        let drawers = self
            .storage
            .scoped_drawers(self.wing.as_deref(), None, 10_000)?;
        if drawers.is_empty() {
            return Ok("## L1 — No memories yet.".to_string());
        }

        let mut scored: Vec<(f64, Drawer)> = drawers
            .into_iter()
            .map(|drawer| (drawer_importance(&drawer), drawer))
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(Self::MAX_DRAWERS);

        let mut by_room: BTreeMap<String, Vec<(f64, Drawer)>> = BTreeMap::new();
        for (importance, drawer) in scored {
            by_room
                .entry(drawer.room.clone())
                .or_default()
                .push((importance, drawer));
        }

        let mut lines = vec!["## L1 — ESSENTIAL STORY".to_string()];
        let mut total_len = 0usize;

        for (room, entries) in by_room {
            let room_line = format!("\n[{}]", room);
            lines.push(room_line.clone());
            total_len += room_line.len();

            for (_, drawer) in entries {
                let source = Path::new(&drawer.source_file)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                let compressed = self
                    .storage
                    .find_compressed_for_raw(&drawer.id)
                    .ok()
                    .flatten();
                let snippet = preferred_snippet(
                    drawer.content.trim(),
                    compressed.as_ref().map(|d| d.content.as_str()),
                    self.prefer_compressed,
                    200,
                );
                let mut entry_line = format!("  - {}", snippet);
                if !source.is_empty() {
                    entry_line.push_str(&format!("  ({})", source));
                }
                if total_len + entry_line.len() > Self::MAX_CHARS {
                    lines.push("  ... (more in L3 search)".to_string());
                    return Ok(lines.join("\n"));
                }
                lines.push(entry_line.clone());
                total_len += entry_line.len();
            }
        }

        Ok(lines.join("\n"))
    }
}

pub struct Layer2<'a> {
    storage: &'a Storage,
}

impl<'a> Layer2<'a> {
    pub fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    pub fn retrieve(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> Result<String> {
        let drawers = self.storage.scoped_drawers(wing, room, n_results)?;
        if drawers.is_empty() {
            let mut label = String::new();
            if let Some(wing) = wing {
                label.push_str(&format!("wing={}", wing));
            }
            if let Some(room) = room {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str(&format!("room={}", room));
            }
            return Ok(format!("No drawers found for {}.", label));
        }

        let mut lines = vec![format!("## L2 — ON-DEMAND ({} drawers)", drawers.len())];
        for drawer in drawers.into_iter().take(n_results) {
            let room_name = drawer.room;
            let source = Path::new(&drawer.source_file)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let mut snippet = drawer.content.trim().replace('\n', " ");
            if snippet.chars().count() > 300 {
                snippet = format!("{}...", snippet.chars().take(297).collect::<String>());
            }
            let mut entry = format!("  [{}] {}", room_name, snippet);
            if !source.is_empty() {
                entry.push_str(&format!("  ({})", source));
            }
            lines.push(entry);
        }
        Ok(lines.join("\n"))
    }
}

pub struct Layer3<'a> {
    storage: &'a Storage,
}

impl<'a> Layer3<'a> {
    pub fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    pub fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> Result<String> {
        let hits = self.storage.search_hybrid(query, wing, room, n_results)?;
        if hits.is_empty() {
            return Ok("No results found.".to_string());
        }

        let mut lines = vec![format!("## L3 — SEARCH RESULTS for \"{}\"", query)];
        for (i, hit) in hits.iter().enumerate() {
            let mut snippet = hit.snippet.replace('\n', " ");
            if snippet.chars().count() > 300 {
                snippet = format!("{}...", snippet.chars().take(297).collect::<String>());
            }
            lines.push(format!(
                "  [{}] {}/{} (sim={:.3})",
                i + 1,
                hit.wing,
                hit.room,
                hit.semantic_score.max(hit.fused_score)
            ));
            lines.push(format!("      {}", snippet));
            let source = Path::new(&hit.source_file)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if !source.is_empty() {
                lines.push(format!("      src: {}", source));
            }
        }
        Ok(lines.join("\n"))
    }
}

pub struct MemoryStack<'a> {
    pub l0: Layer0,
    pub l1: Layer1<'a>,
    pub l2: Layer2<'a>,
    pub l3: Layer3<'a>,
}

impl<'a> MemoryStack<'a> {
    pub fn new(config: &AppConfig, storage: &'a Storage, wing: Option<&str>) -> Self {
        let prefer_compressed = std::env::var("MEMPALACE_PREFER_COMPRESSED")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        Self {
            l0: Layer0::new(&config.identity_file),
            l1: Layer1::new(storage, wing, prefer_compressed),
            l2: Layer2::new(storage),
            l3: Layer3::new(storage),
        }
    }

    pub fn wake_up(&self) -> Result<String> {
        Ok(format!("{}\n\n{}", self.l0.render(), self.l1.generate()?))
    }

    pub fn recall(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> Result<String> {
        self.l2.retrieve(wing, room, n_results)
    }

    pub fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> Result<String> {
        self.l3.search(query, wing, room, n_results)
    }
}

fn drawer_importance(drawer: &Drawer) -> f64 {
    drawer
        .importance
        .or(drawer.emotional_weight)
        .or(drawer.weight)
        .unwrap_or(3.0)
}
