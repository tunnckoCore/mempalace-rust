use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct ExtractedMemory {
    pub content: String,
    pub memory_type: String,
    pub chunk_index: usize,
}

pub fn extract_memories(text: &str, min_confidence: f32) -> Vec<ExtractedMemory> {
    let segments = split_into_segments(text);
    let marker_sets = marker_sets();
    let mut out = Vec::new();

    for segment in segments {
        if segment.trim().len() < 20 {
            continue;
        }
        let prose = extract_prose(&segment);
        let mut scores: HashMap<&str, f32> = HashMap::new();
        for (kind, markers) in &marker_sets {
            let score = score_markers(&prose, markers);
            if score > 0.0 {
                scores.insert(kind, score);
            }
        }
        if scores.is_empty() {
            continue;
        }
        let mut best = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| (*k).to_string())
            .unwrap();
        best = disambiguate(&best, &prose, &scores);
        let length_bonus = if segment.len() > 500 {
            2.0
        } else if segment.len() > 200 {
            1.0
        } else {
            0.0
        };
        let confidence =
            ((scores.get(best.as_str()).copied().unwrap_or(0.0) + length_bonus) / 5.0).min(1.0);
        if confidence < min_confidence {
            continue;
        }
        out.push(ExtractedMemory {
            content: segment.trim().to_string(),
            memory_type: best,
            chunk_index: out.len(),
        });
    }

    out
}

fn marker_sets() -> HashMap<&'static str, Vec<Regex>> {
    let raw: &[(&str, &[&str])] = &[
        (
            "decision",
            &[
                r"\blet'?s (use|go with|try|pick|choose|switch to)\b",
                r"\bwe (should|decided|chose|went with|picked|settled on)\b",
                r"\binstead of\b",
                r"\bbecause\b",
                r"\btrade-?off\b",
                r"\barchitecture\b",
                r"\bapproach\b",
                r"\bframework\b",
                r"\bconfigure\b",
            ],
        ),
        (
            "preference",
            &[
                r"\bi prefer\b",
                r"\balways use\b",
                r"\bnever use\b",
                r"\bdon'?t (ever |like to )?(use|do|mock|stub|import)\b",
                r"\bi like (to|when|how)\b",
                r"\bi hate (when|how|it when)\b",
                r"\bmy (rule|preference|style|convention) is\b",
                r"\bwe (always|never)\b",
            ],
        ),
        (
            "milestone",
            &[
                r"\bit works\b",
                r"\bit worked\b",
                r"\bgot it working\b",
                r"\bfixed\b",
                r"\bsolved\b",
                r"\bbreakthrough\b",
                r"\bfigured (it )?out\b",
                r"\bfinally\b",
                r"\bdiscovered\b",
                r"\brealized\b",
                r"\bthe key (is|was|insight)\b",
                r"\bbuilt\b",
                r"\bimplemented\b",
                r"\bdeployed\b",
            ],
        ),
        (
            "problem",
            &[
                r"\b(bug|error|crash|fail|broke|broken|issue|problem)\b",
                r"\bdoesn'?t work\b",
                r"\bnot working\b",
                r"\broot cause\b",
                r"\bthe fix (is|was)\b",
                r"\bworkaround\b",
                r"\bresolved\b",
                r"\bpatched\b",
            ],
        ),
        (
            "emotional",
            &[
                r"\blove\b",
                r"\bscared\b",
                r"\bafraid\b",
                r"\bproud\b",
                r"\bhurt\b",
                r"\bhappy\b",
                r"\bsad\b",
                r"\bcry\b",
                r"\bgrateful\b",
                r"\bangry\b",
                r"\bworried\b",
                r"i feel",
                r"i'm scared",
                r"i love you",
                r"i'm sorry",
                r"i wish",
                r"i miss",
            ],
        ),
    ];
    let mut out = HashMap::new();
    for (kind, patterns) in raw {
        out.insert(
            *kind,
            patterns
                .iter()
                .map(|p| Regex::new(p).expect("valid extractor regex"))
                .collect(),
        );
    }
    out
}

fn split_into_segments(text: &str) -> Vec<String> {
    let lines: Vec<_> = text.lines().collect();
    let turn_patterns = vec![
        Regex::new(r"^>\s").unwrap(),
        Regex::new(r"^(Human|User|Q)\s*:").unwrap(),
        Regex::new(r"^(Assistant|AI|A|Claude|ChatGPT)\s*:").unwrap(),
    ];
    let turn_count = lines
        .iter()
        .filter(|line| turn_patterns.iter().any(|pat| pat.is_match(line.trim())))
        .count();
    if turn_count >= 3 {
        return split_by_turns(&lines, &turn_patterns);
    }
    let paragraphs: Vec<_> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(ToString::to_string)
        .collect();
    if paragraphs.len() <= 1 && lines.len() > 20 {
        return lines
            .chunks(25)
            .map(|group| group.join("\n"))
            .filter(|s| !s.trim().is_empty())
            .collect();
    }
    paragraphs
}

fn split_by_turns(lines: &[&str], turn_patterns: &[Regex]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    for line in lines {
        let stripped = line.trim();
        let is_turn = turn_patterns.iter().any(|pat| pat.is_match(stripped));
        if is_turn && !current.is_empty() {
            segments.push(current.join("\n"));
            current = vec![(*line).to_string()];
        } else {
            current.push((*line).to_string());
        }
    }
    if !current.is_empty() {
        segments.push(current.join("\n"));
    }
    segments
}

fn extract_prose(text: &str) -> String {
    let code_patterns = [
        Regex::new(r"^\s*[\$#]\s").unwrap(),
        Regex::new(r"^\s*(cd|source|echo|export|pip|npm|git|python|bash|curl|wget|mkdir|rm|cp|mv|ls|cat|grep|find|chmod|sudo|brew|docker)\s").unwrap(),
        Regex::new(r"^\s*```").unwrap(),
        Regex::new(r"^\s*(import|from|def|class|function|const|let|var|return)\s").unwrap(),
    ];
    let mut prose = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if !code_patterns.iter().any(|pat| pat.is_match(trimmed)) {
            prose.push(line);
        }
    }
    let joined = prose.join("\n").trim().to_string();
    if joined.is_empty() {
        text.to_string()
    } else {
        joined
    }
}

fn score_markers(text: &str, markers: &[Regex]) -> f32 {
    let lower = text.to_lowercase();
    markers
        .iter()
        .map(|re| re.find_iter(&lower).count() as f32)
        .sum()
}

fn disambiguate(memory_type: &str, text: &str, scores: &HashMap<&str, f32>) -> String {
    let lower = text.to_lowercase();
    let positive: HashSet<&str> = [
        "proud",
        "joy",
        "happy",
        "love",
        "beautiful",
        "amazing",
        "wonderful",
        "excited",
        "grateful",
        "breakthrough",
        "success",
        "works",
        "working",
        "solved",
        "fixed",
    ]
    .into_iter()
    .collect();
    let negative: HashSet<&str> = [
        "bug", "error", "crash", "fail", "failed", "broken", "issue", "problem", "wrong", "stuck",
        "panic", "disaster",
    ]
    .into_iter()
    .collect();
    let words: HashSet<String> = Regex::new(r"\b\w+\b")
        .unwrap()
        .find_iter(&lower)
        .map(|m| m.as_str().to_string())
        .collect();
    let pos = positive.iter().filter(|w| words.contains(**w)).count();
    let neg = negative.iter().filter(|w| words.contains(**w)).count();
    let sentiment = match pos.cmp(&neg) {
        std::cmp::Ordering::Greater => "positive",
        std::cmp::Ordering::Less => "negative",
        std::cmp::Ordering::Equal => "neutral",
    };
    let has_resolution = [
        "fixed",
        "solved",
        "resolved",
        "patched",
        "got it working",
        "it works",
        "nailed it",
        "figured it out",
        "the fix",
        "the solution",
    ]
    .iter()
    .any(|p| lower.contains(p));
    if memory_type == "problem" && has_resolution {
        if scores.get("emotional").copied().unwrap_or(0.0) > 0.0 && sentiment == "positive" {
            return "emotional".to_string();
        }
        return "milestone".to_string();
    }
    if memory_type == "problem" && sentiment == "positive" {
        if scores.get("milestone").copied().unwrap_or(0.0) > 0.0 {
            return "milestone".to_string();
        }
        if scores.get("emotional").copied().unwrap_or(0.0) > 0.0 {
            return "emotional".to_string();
        }
    }
    memory_type.to_string()
}
