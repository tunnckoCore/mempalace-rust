use regex::Regex;

pub fn normalize_query_for_fts(input: &str) -> Option<String> {
    let token_re = Regex::new(r"[A-Za-z0-9_\-]+").expect("query token regex should compile");
    let mut terms = Vec::new();
    for cap in token_re.find_iter(input) {
        let token = cap.as_str();
        if token.len() >= 2 {
            terms.push(format!("\"{}\"*", token.replace('"', "")));
        }
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('_');
            last_dash = true;
        }
    }
    out.trim_matches('_').to_string()
}

pub fn chunk_text(
    content: &str,
    chunk_size: usize,
    overlap: usize,
    min_size: usize,
) -> Vec<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        let mut end = (start + chunk_size).min(chars.len());
        if end < chars.len() {
            let window: String = chars[start..end].iter().collect();
            if let Some(pos) = window.rfind("\n\n") {
                if pos > chunk_size / 2 {
                    end = start + window[..pos].chars().count();
                }
            } else if let Some(pos) = window.rfind('\n') {
                if pos > chunk_size / 2 {
                    end = start + window[..pos].chars().count();
                }
            }
        }

        let chunk: String = chars[start..end].iter().collect();
        let chunk = chunk.trim();
        if chunk.chars().count() >= min_size {
            chunks.push(chunk.to_string());
        }
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }
    chunks
}
