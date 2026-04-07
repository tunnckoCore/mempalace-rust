use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub const AAAK_SPEC: &str = "AAAK is a compressed memory dialect that MemPalace uses for efficient storage.\nFORMAT: header wing|room|date|source then zettel-like compact lines.\nENTITIES: short uppercase codes. EMOTIONS: compact markers. FLAGS: ORIGIN CORE SENSITIVE PIVOT GENESIS DECISION TECHNICAL.";

#[derive(Default, Clone)]
pub struct Dialect {
    entity_codes: HashMap<String, String>,
    skip_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompressionStats {
    pub original_chars: usize,
    pub compressed_chars: usize,
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub ratio: f32,
}

impl Dialect {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn compress(&self, text: &str, metadata: Option<&HashMap<String, String>>) -> String {
        let entities = self.detect_entities_in_text(text);
        let entity_str = if entities.is_empty() {
            "???".to_string()
        } else {
            entities.join("+")
        };
        let topics = self.extract_topics(text, 3);
        let topic_str = if topics.is_empty() {
            "misc".to_string()
        } else {
            topics.join("_")
        };
        let quote = self.extract_key_sentence(text);
        let emotions = self.detect_emotions(text);
        let flags = self.detect_flags(text);
        let mut lines = Vec::new();
        if let Some(meta) = metadata {
            let source = meta.get("source_file").cloned().unwrap_or_default();
            let wing = meta.get("wing").cloned().unwrap_or_else(|| "?".to_string());
            let room = meta.get("room").cloned().unwrap_or_else(|| "?".to_string());
            let date = meta.get("date").cloned().unwrap_or_else(|| "?".to_string());
            let stem = if source.is_empty() {
                "?".to_string()
            } else {
                Path::new(&source)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string()
            };
            lines.push(format!("{}|{}|{}|{}", wing, room, date, stem));
        }
        let mut parts = vec![format!("0:{}", entity_str), topic_str];
        if !quote.is_empty() {
            parts.push(format!("\"{}\"", quote));
        }
        if !emotions.is_empty() {
            parts.push(emotions.join("+"));
        }
        if !flags.is_empty() {
            parts.push(flags.join("+"));
        }
        lines.push(parts.join("|"));
        lines.join("\n")
    }

    pub fn compression_stats(&self, original: &str, compressed: &str) -> CompressionStats {
        CompressionStats {
            original_chars: original.len(),
            compressed_chars: compressed.len(),
            original_tokens: count_tokens(original),
            compressed_tokens: count_tokens(compressed),
            ratio: (original.len() as f32) / (compressed.len().max(1) as f32),
        }
    }

    fn detect_emotions(&self, text: &str) -> Vec<String> {
        let signals = [
            ("decided", "determ"),
            ("prefer", "convict"),
            ("worried", "anx"),
            ("excited", "excite"),
            ("frustrated", "frust"),
            ("confused", "confuse"),
            ("love", "love"),
            ("hate", "rage"),
            ("hope", "hope"),
            ("fear", "fear"),
            ("trust", "trust"),
            ("happy", "joy"),
            ("sad", "grief"),
            ("surprised", "surprise"),
            ("grateful", "grat"),
            ("curious", "curious"),
            ("wonder", "wonder"),
            ("anxious", "anx"),
            ("relieved", "relief"),
            ("satisf", "satis"),
        ];
        let lower = text.to_lowercase();
        let mut out = Vec::new();
        for (kw, code) in signals {
            if lower.contains(kw) && !out.iter().any(|s| s == code) {
                out.push(code.to_string());
            }
            if out.len() >= 3 {
                break;
            }
        }
        out
    }

    fn detect_flags(&self, text: &str) -> Vec<String> {
        let signals = [
            ("decided", "DECISION"),
            ("chose", "DECISION"),
            ("switched", "DECISION"),
            ("migrated", "DECISION"),
            ("replaced", "DECISION"),
            ("because", "DECISION"),
            ("founded", "ORIGIN"),
            ("created", "ORIGIN"),
            ("started", "ORIGIN"),
            ("launched", "ORIGIN"),
            ("core", "CORE"),
            ("fundamental", "CORE"),
            ("turning point", "PIVOT"),
            ("breakthrough", "PIVOT"),
            ("api", "TECHNICAL"),
            ("database", "TECHNICAL"),
            ("architecture", "TECHNICAL"),
            ("deploy", "TECHNICAL"),
            ("framework", "TECHNICAL"),
            ("server", "TECHNICAL"),
        ];
        let lower = text.to_lowercase();
        let mut out = Vec::new();
        for (kw, flag) in signals {
            if lower.contains(kw) && !out.iter().any(|s| s == flag) {
                out.push(flag.to_string());
            }
            if out.len() >= 3 {
                break;
            }
        }
        out
    }

    fn extract_topics(&self, text: &str, max_topics: usize) -> Vec<String> {
        let re = Regex::new(r"[A-Za-z][A-Za-z_-]{2,}").expect("topic regex");
        let stop_words: HashSet<&str> = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "to", "of", "in", "for",
            "on", "with", "at", "by", "from", "as", "and", "but", "or", "if", "that", "this",
            "these", "those", "it", "its", "i", "we", "you", "they", "my", "your", "our", "their",
            "what", "which", "who", "because", "like", "use", "using", "make", "made", "thing",
            "things",
        ]
        .into_iter()
        .collect();
        let mut freq: HashMap<String, usize> = HashMap::new();
        for mat in re.find_iter(text) {
            let w = mat.as_str();
            let wl = w.to_lowercase();
            if stop_words.contains(wl.as_str()) || wl.len() < 3 {
                continue;
            }
            *freq.entry(wl.clone()).or_insert(0) += 1;
            if w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                *freq.entry(wl.clone()).or_insert(0) += 2;
            }
            if w.contains('_') || w.contains('-') || w.chars().skip(1).any(|c| c.is_uppercase()) {
                *freq.entry(wl).or_insert(0) += 2;
            }
        }
        let mut ranked: Vec<_> = freq.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(max_topics)
            .map(|(w, _)| w)
            .collect()
    }

    fn extract_key_sentence(&self, text: &str) -> String {
        let splitter = Regex::new(r"[.!?\n]+").expect("sentence regex");
        let mut best = String::new();
        let mut best_score = i32::MIN;
        for sentence in splitter.split(text).map(str::trim).filter(|s| s.len() > 10) {
            let lower = sentence.to_lowercase();
            let mut score = 0i32;
            for kw in [
                "decided",
                "because",
                "instead",
                "prefer",
                "switched",
                "chose",
                "realized",
                "important",
                "key",
                "critical",
                "discovered",
                "learned",
                "solution",
                "reason",
                "breakthrough",
                "insight",
            ] {
                if lower.contains(kw) {
                    score += 2;
                }
            }
            if sentence.len() < 80 {
                score += 1;
            }
            if sentence.len() < 40 {
                score += 1;
            }
            if sentence.len() > 150 {
                score -= 2;
            }
            if score > best_score {
                best_score = score;
                best = sentence.to_string();
            }
        }
        if best.len() > 55 {
            format!("{}...", &best.chars().take(52).collect::<String>())
        } else {
            best
        }
    }

    fn detect_entities_in_text(&self, text: &str) -> Vec<String> {
        let lower = text.to_lowercase();
        let mut found = Vec::new();
        for (name, code) in &self.entity_codes {
            if !name.chars().all(|c| c.is_lowercase())
                && lower.contains(&name.to_lowercase())
                && !found.contains(code)
            {
                found.push(code.clone());
            }
        }
        if !found.is_empty() {
            return found.into_iter().take(3).collect();
        }
        let re = Regex::new(r"\b[A-Z][a-z]{1,}\b").expect("entity regex");
        for mat in re.find_iter(text) {
            let name = mat.as_str();
            if self
                .skip_names
                .iter()
                .any(|n| name.to_lowercase().contains(n))
            {
                continue;
            }
            let code = name.chars().take(3).collect::<String>().to_uppercase();
            if !found.contains(&code) {
                found.push(code);
            }
            if found.len() >= 3 {
                break;
            }
        }
        found
    }
}

pub fn count_tokens(text: &str) -> usize {
    text.len() / 4
}
