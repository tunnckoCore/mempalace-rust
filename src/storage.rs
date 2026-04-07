use crate::embedding::{
    cosine_similarity, decode_vector, embed_text, embed_text_with_preference, encode_vector,
    named_entityish_boost, phrase_overlap_score, EmbeddingBackend, EmbeddingPreference,
};
use crate::limits::MAX_QUERY_CHARS;
use crate::search::normalize_query_for_fts;
use crate::storage_types::SourceRefreshPlanOwned;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Drawer {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub source_file: String,
    pub chunk_index: i64,
    pub added_by: String,
    pub filed_at: String,
    pub content: String,
    pub hall: Option<String>,
    pub date: Option<String>,
    pub drawer_type: String,
    pub source_hash: Option<String>,
    pub active: bool,
    pub importance: Option<f64>,
    pub emotional_weight: Option<f64>,
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub source_file: String,
    pub chunk_index: i64,
    pub added_by: String,
    pub filed_at: String,
    pub snippet: String,
    pub lexical_score: f64,
    pub semantic_score: f64,
    pub heuristic_score: f64,
    pub fused_score: f64,
    pub drawer_type: String,
    pub embedding_backend: String,
    pub parent_drawer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub total_drawers: i64,
    pub by_wing: BTreeMap<String, BTreeMap<String, i64>>,
    pub artifacts_by_type: BTreeMap<String, i64>,
}

#[derive(Debug, Clone)]
pub struct DrawerInput<'a> {
    pub id: &'a str,
    pub wing: &'a str,
    pub room: &'a str,
    pub source_file: &'a str,
    pub chunk_index: i64,
    pub added_by: &'a str,
    pub content: &'a str,
    pub hall: Option<&'a str>,
    pub date: Option<&'a str>,
    pub drawer_type: &'a str,
    pub source_hash: Option<&'a str>,
    pub importance: Option<f64>,
    pub emotional_weight: Option<f64>,
    pub weight: Option<f64>,
}

pub struct SourceRefreshPlan<'a> {
    pub source_file: &'a str,
    pub source_hash: &'a str,
    pub drawers: Vec<DrawerInput<'a>>,
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(palace_path: &Path) -> Result<Self> {
        fs::create_dir_all(palace_path)
            .with_context(|| format!("creating palace dir {}", palace_path.display()))?;
        let db_path = palace_path.join("mempalace.sqlite3");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("opening sqlite db {}", db_path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS drawers (
                id TEXT PRIMARY KEY,
                wing TEXT NOT NULL,
                room TEXT NOT NULL,
                source_file TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                added_by TEXT NOT NULL,
                filed_at TEXT NOT NULL,
                content TEXT NOT NULL,
                hall TEXT,
                date TEXT,
                drawer_type TEXT NOT NULL DEFAULT 'drawer',
                source_hash TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                importance REAL,
                emotional_weight REAL,
                weight REAL
            );
            CREATE TABLE IF NOT EXISTS source_revisions (
                source_file TEXT PRIMARY KEY,
                source_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS vectors (
                drawer_id TEXT PRIMARY KEY,
                embedding BLOB NOT NULL,
                FOREIGN KEY(drawer_id) REFERENCES drawers(id) ON DELETE CASCADE
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_drawers_source_chunk_type
                ON drawers(source_file, chunk_index, wing, room, drawer_type, active);
            CREATE INDEX IF NOT EXISTS idx_drawers_wing ON drawers(wing);
            CREATE INDEX IF NOT EXISTS idx_drawers_room ON drawers(room);
            CREATE INDEX IF NOT EXISTS idx_drawers_source_file ON drawers(source_file);
            CREATE INDEX IF NOT EXISTS idx_drawers_type ON drawers(drawer_type);
            CREATE VIRTUAL TABLE IF NOT EXISTS drawers_fts USING fts5(
                content,
                wing UNINDEXED,
                room UNINDEXED,
                source_file UNINDEXED,
                id UNINDEXED,
                drawer_type UNINDEXED,
                tokenize = 'unicode61'
            );
            CREATE TRIGGER IF NOT EXISTS drawers_ai AFTER INSERT ON drawers WHEN new.active = 1 BEGIN
                INSERT INTO drawers_fts(rowid, content, wing, room, source_file, id, drawer_type)
                VALUES (new.rowid, new.content, new.wing, new.room, new.source_file, new.id, new.drawer_type);
            END;
            CREATE TRIGGER IF NOT EXISTS drawers_ad AFTER DELETE ON drawers BEGIN
                INSERT INTO drawers_fts(drawers_fts, rowid, content, wing, room, source_file, id, drawer_type)
                VALUES('delete', old.rowid, old.content, old.wing, old.room, old.source_file, old.id, old.drawer_type);
            END;
            CREATE TRIGGER IF NOT EXISTS drawers_au AFTER UPDATE ON drawers BEGIN
                INSERT INTO drawers_fts(drawers_fts, rowid, content, wing, room, source_file, id, drawer_type)
                VALUES('delete', old.rowid, old.content, old.wing, old.room, old.source_file, old.id, old.drawer_type);
                INSERT INTO drawers_fts(rowid, content, wing, room, source_file, id, drawer_type)
                SELECT new.rowid, new.content, new.wing, new.room, new.source_file, new.id, new.drawer_type
                WHERE new.active = 1;
            END;
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn source_revision(&self, source_file: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT source_hash FROM source_revisions WHERE source_file = ?1",
                [source_file],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn source_is_current(&self, source_file: &str, source_hash: &str) -> Result<bool> {
        Ok(self.source_revision(source_file)?.as_deref() == Some(source_hash))
    }

    pub fn retire_source(&self, source_file: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE drawers SET active = 0 WHERE source_file = ?1",
            [source_file],
        )?;
        Ok(())
    }

    pub fn update_source_revision(&self, source_file: &str, source_hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO source_revisions(source_file, source_hash, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(source_file) DO UPDATE SET source_hash=excluded.source_hash, updated_at=excluded.updated_at",
            params![source_file, source_hash, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn refresh_source(&mut self, plan: SourceRefreshPlan<'_>) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let inserted = refresh_source_in_tx(&tx, plan)?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn refresh_source_owned(&mut self, plan: SourceRefreshPlanOwned) -> Result<usize> {
        let tx = self.conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        let mut inserted = 0usize;
        for input in &plan.drawers {
            let changed = tx.execute(
                "INSERT OR REPLACE INTO drawers(id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?14, ?15)",
                params![input.id, input.wing, input.room, input.source_file, input.chunk_index, input.added_by, now, input.content, input.hall, input.date, input.drawer_type, input.source_hash, input.importance, input.emotional_weight, input.weight],
            )?;
            let embedding = encode_vector(&embed_text(&input.content).vector);
            tx.execute(
                "INSERT OR REPLACE INTO vectors(drawer_id, embedding) VALUES (?1, ?2)",
                params![input.id, embedding],
            )?;
            if changed > 0 {
                inserted += 1;
            }
        }
        tx.execute(
            "UPDATE drawers SET active = 0 WHERE source_file = ?1 AND source_hash IS NOT ?2",
            params![plan.source_file, plan.source_hash],
        )?;
        tx.execute(
            "INSERT INTO source_revisions(source_file, source_hash, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(source_file) DO UPDATE SET source_hash=excluded.source_hash, updated_at=excluded.updated_at",
            params![plan.source_file, plan.source_hash, now],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    pub fn add_drawer(&self, input: DrawerInput<'_>) -> Result<bool> {
        let filed_at = Utc::now().to_rfc3339();
        let changed = self.conn.execute(
            "INSERT OR REPLACE INTO drawers(id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?14, ?15)",
            params![input.id, input.wing, input.room, input.source_file, input.chunk_index, input.added_by, filed_at, input.content, input.hall, input.date, input.drawer_type, input.source_hash, input.importance, input.emotional_weight, input.weight],
        )?;
        let embedding = encode_vector(&embed_text(input.content).vector);
        self.conn.execute(
            "INSERT OR REPLACE INTO vectors(drawer_id, embedding) VALUES (?1, ?2)",
            params![input.id, embedding],
        )?;
        Ok(changed > 0)
    }

    pub fn all_drawers(&self) -> Result<Vec<Drawer>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight
             FROM drawers WHERE active = 1 ORDER BY filed_at DESC"
        )?;
        let rows = stmt.query_map([], map_drawer_full)?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn status(&self) -> Result<StatusReport> {
        let total_drawers: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM drawers WHERE active = 1", [], |row| {
                    row.get(0)
                })?;
        let mut stmt = self.conn.prepare(
            "SELECT wing, room, COUNT(*) FROM drawers WHERE active = 1 GROUP BY wing, room ORDER BY wing, room",
        )?;
        let mut rows = stmt.query([])?;
        let mut by_wing: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        while let Some(row) = rows.next()? {
            let wing: String = row.get(0)?;
            let room: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            by_wing.entry(wing).or_default().insert(room, count);
        }
        let mut stmt = self.conn.prepare(
            "SELECT drawer_type, COUNT(*) FROM drawers WHERE active = 1 GROUP BY drawer_type ORDER BY drawer_type"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let artifacts_by_type = rows.collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        Ok(StatusReport {
            total_drawers,
            by_wing,
            artifacts_by_type,
        })
    }

    pub fn top_wings(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT wing, COUNT(*) AS c FROM drawers WHERE active = 1 GROUP BY wing ORDER BY c DESC, wing ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn sample_for_wing(&self, wing: Option<&str>, limit: usize) -> Result<Vec<Drawer>> {
        self.scoped_drawers(wing, None, limit)
    }

    pub fn scoped_drawers(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>> {
        let sql = match (wing, room) {
            (Some(_), Some(_)) => "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight FROM drawers WHERE active = 1 AND wing = ?1 AND room = ?2 ORDER BY filed_at DESC, chunk_index ASC LIMIT ?3",
            (Some(_), None) => "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight FROM drawers WHERE active = 1 AND wing = ?1 ORDER BY filed_at DESC, chunk_index ASC LIMIT ?2",
            (None, Some(_)) => "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight FROM drawers WHERE active = 1 AND room = ?1 ORDER BY filed_at DESC, chunk_index ASC LIMIT ?2",
            (None, None) => "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight FROM drawers WHERE active = 1 ORDER BY filed_at DESC, chunk_index ASC LIMIT ?1",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match (wing, room) {
            (Some(wing), Some(room)) => {
                stmt.query_map(params![wing, room, limit as i64], map_drawer_full)?
            }
            (Some(wing), None) => stmt.query_map(params![wing, limit as i64], map_drawer_full)?,
            (None, Some(room)) => stmt.query_map(params![room, limit as i64], map_drawer_full)?,
            (None, None) => stmt.query_map(params![limit as i64], map_drawer_full)?,
        };
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn search_hybrid(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let capped_limit = limit.min(50);
        let lexical =
            self.lexical_search(query, wing, room, capped_limit.saturating_mul(5).max(20))?;
        let candidate_ids = if lexical.is_empty() {
            None
        } else {
            Some(
                lexical
                    .iter()
                    .take(200)
                    .map(|hit| hit.id.clone())
                    .collect::<Vec<_>>(),
            )
        };
        let semantic = self.semantic_search(
            query,
            wing,
            room,
            capped_limit.saturating_mul(5).max(20),
            candidate_ids.as_deref(),
        )?;
        let mut merged: HashMap<String, SearchHit> = HashMap::new();

        for (rank, hit) in lexical.into_iter().enumerate() {
            let entry = merged.entry(hit.id.clone()).or_insert(hit.clone());
            entry.lexical_score = entry.lexical_score.max(hit.lexical_score);
            entry.heuristic_score = entry.heuristic_score.max(hit.heuristic_score);
            entry.fused_score += 0.8 / (rank as f64 + 1.0);
        }
        for (rank, hit) in semantic.into_iter().enumerate() {
            let entry = merged.entry(hit.id.clone()).or_insert(hit.clone());
            entry.semantic_score = entry.semantic_score.max(hit.semantic_score);
            entry.heuristic_score = entry.heuristic_score.max(hit.heuristic_score);
            entry.fused_score += 1.0 / (rank as f64 + 1.0);
            if entry.snippet.is_empty() {
                entry.snippet = hit.snippet;
            }
            if entry.embedding_backend.is_empty() {
                entry.embedding_backend = hit.embedding_backend;
            }
        }

        for entry in merged.values_mut() {
            entry.fused_score += entry.semantic_score * 0.45;
            entry.fused_score += entry.lexical_score * 0.30;
            entry.fused_score += entry.heuristic_score * 0.25;
            if is_derived_type(&entry.drawer_type) {
                entry.fused_score += 0.10;
            }
        }

        let mut values: Vec<_> = merged.into_values().collect();
        values = self.resolve_artifact_hits(values)?;
        values.sort_by(|a, b| {
            b.fused_score
                .partial_cmp(&a.fused_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        values.truncate(capped_limit);
        Ok(values)
    }

    pub fn lexical_debug_search(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        Ok(self
            .lexical_search(query, None, None, limit)?
            .into_iter()
            .map(|hit| hit.id)
            .collect())
    }

    pub fn semantic_debug_search(
        &self,
        query: &str,
        preference: EmbeddingPreference,
        limit: usize,
    ) -> Result<Vec<String>> {
        let query_embedding = embed_text_with_preference(query, preference);
        let query_vec = query_embedding.vector.clone();
        let mut stmt = self.conn.prepare(
            "SELECT d.id, v.embedding, d.content FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut scored = Vec::new();
        for row in rows {
            let (id, embedding_bytes, content) = row?;
            let embedding = decode_vector(&embedding_bytes);
            let semantic = cosine_similarity(&query_vec, &embedding) as f64;
            let heuristic = (phrase_overlap_score(query, &content) * 0.7
                + named_entityish_boost(query, &content) * 0.3) as f64;
            scored.push((id, semantic * 0.75 + heuristic * 0.25));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(id, _)| id).collect())
    }

    fn lexical_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let Some(fts_query) = normalize_query_for_fts(query) else {
            return Ok(Vec::new());
        };
        let query_lower = query.to_lowercase();
        let sql = match (wing, room) {
            (Some(_), Some(_)) => {
                "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at,
                        snippet(drawers_fts, 0, '[', ']', ' … ', 20) AS snippet,
                        bm25(drawers_fts) AS score, d.drawer_type, d.content
                 FROM drawers_fts
                 JOIN drawers d ON d.rowid = drawers_fts.rowid
                 WHERE drawers_fts MATCH ?1 AND d.active = 1 AND d.wing = ?2 AND d.room = ?3
                 ORDER BY score LIMIT ?4"
            }
            (Some(_), None) => {
                "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at,
                        snippet(drawers_fts, 0, '[', ']', ' … ', 20) AS snippet,
                        bm25(drawers_fts) AS score, d.drawer_type, d.content
                 FROM drawers_fts
                 JOIN drawers d ON d.rowid = drawers_fts.rowid
                 WHERE drawers_fts MATCH ?1 AND d.active = 1 AND d.wing = ?2
                 ORDER BY score LIMIT ?3"
            }
            (None, Some(_)) => {
                "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at,
                        snippet(drawers_fts, 0, '[', ']', ' … ', 20) AS snippet,
                        bm25(drawers_fts) AS score, d.drawer_type, d.content
                 FROM drawers_fts
                 JOIN drawers d ON d.rowid = drawers_fts.rowid
                 WHERE drawers_fts MATCH ?1 AND d.active = 1 AND d.room = ?2
                 ORDER BY score LIMIT ?3"
            }
            (None, None) => {
                "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at,
                        snippet(drawers_fts, 0, '[', ']', ' … ', 20) AS snippet,
                        bm25(drawers_fts) AS score, d.drawer_type, d.content
                 FROM drawers_fts
                 JOIN drawers d ON d.rowid = drawers_fts.rowid
                 WHERE drawers_fts MATCH ?1 AND d.active = 1
                 ORDER BY score LIMIT ?2"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mapper = move |row: &rusqlite::Row<'_>| {
            let raw_score: f64 = row.get(8)?;
            let full_content: String = row.get(10)?;
            let heuristic = (phrase_overlap_score(&query_lower, &full_content) * 0.7
                + named_entityish_boost(query, &full_content) * 0.3)
                as f64;
            Ok(SearchHit {
                id: row.get(0)?,
                wing: row.get(1)?,
                room: row.get(2)?,
                source_file: row.get(3)?,
                chunk_index: row.get(4)?,
                added_by: row.get(5)?,
                filed_at: row.get(6)?,
                snippet: row.get(7)?,
                lexical_score: 1.0 / (1.0 + raw_score.abs()),
                semantic_score: 0.0,
                heuristic_score: heuristic,
                fused_score: 0.0,
                drawer_type: row.get(9)?,
                embedding_backend: String::new(),
                parent_drawer_id: None,
            })
        };
        let rows = match (wing, room) {
            (Some(wing), Some(room)) => {
                stmt.query_map(params![fts_query, wing, room, limit as i64], mapper)?
            }
            (Some(wing), None) => stmt.query_map(params![fts_query, wing, limit as i64], mapper)?,
            (None, Some(room)) => stmt.query_map(params![fts_query, room, limit as i64], mapper)?,
            (None, None) => stmt.query_map(params![fts_query, limit as i64], mapper)?,
        };
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    fn semantic_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
        candidate_ids: Option<&[String]>,
    ) -> Result<Vec<SearchHit>> {
        if let Some(candidate_ids) = candidate_ids {
            return self.semantic_search_candidates(query, limit, candidate_ids);
        }
        let query_embedding = embed_text(query);
        let query_vec = query_embedding.vector.clone();
        let sql_limit = if query.chars().count() > MAX_QUERY_CHARS / 2 {
            2_000
        } else {
            10_000
        };
        let sql = match (wing, room) {
            (Some(_), Some(_)) => "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at, substr(d.content, 1, 300), d.drawer_type, v.embedding FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1 AND d.wing = ?1 AND d.room = ?2 LIMIT ?3",
            (Some(_), None) => "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at, substr(d.content, 1, 300), d.drawer_type, v.embedding FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1 AND d.wing = ?1 LIMIT ?2",
            (None, Some(_)) => "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at, substr(d.content, 1, 300), d.drawer_type, v.embedding FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1 AND d.room = ?1 LIMIT ?2",
            (None, None) => "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at, substr(d.content, 1, 300), d.drawer_type, v.embedding FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1 LIMIT ?1",
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = match (wing, room) {
            (Some(wing), Some(room)) => {
                stmt.query_map(params![wing, room, sql_limit], map_semantic_row)?
            }
            (Some(wing), None) => stmt.query_map(params![wing, sql_limit], map_semantic_row)?,
            (None, Some(room)) => stmt.query_map(params![room, sql_limit], map_semantic_row)?,
            (None, None) => stmt.query_map(params![sql_limit], map_semantic_row)?,
        };
        let mut hits = Vec::new();
        for row in rows {
            let (mut hit, embedding_bytes, content_preview) = row?;
            let embedding = decode_vector(&embedding_bytes);
            hit.semantic_score = cosine_similarity(&query_vec, &embedding) as f64;
            hit.heuristic_score = (phrase_overlap_score(query, &content_preview) * 0.7
                + named_entityish_boost(query, &content_preview) * 0.3)
                as f64;
            hit.embedding_backend = match query_embedding.backend {
                EmbeddingBackend::OnnxLocal => "onnx_local".to_string(),
                EmbeddingBackend::StrongLocal => "strong_local".to_string(),
                EmbeddingBackend::LexicalFallback => "lexical_fallback".to_string(),
            };
            hits.push(hit);
        }
        hits.sort_by(|a, b| {
            let score_a = a.semantic_score * 0.75 + a.heuristic_score * 0.25;
            let score_b = b.semantic_score * 0.75 + b.heuristic_score * 0.25;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        self.search_hybrid(query, wing, room, limit)
    }

    fn semantic_search_candidates(
        &self,
        query: &str,
        limit: usize,
        candidate_ids: &[String],
    ) -> Result<Vec<SearchHit>> {
        let query_embedding = embed_text(query);
        let query_vec = query_embedding.vector.clone();
        let mut hits = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT d.id, d.wing, d.room, d.source_file, d.chunk_index, d.added_by, d.filed_at, substr(d.content, 1, 300), d.drawer_type, v.embedding FROM drawers d JOIN vectors v ON v.drawer_id = d.id WHERE d.active = 1 AND d.id = ?1"
        )?;
        for candidate_id in candidate_ids.iter().take(200) {
            if let Some((mut hit, embedding_bytes, content_preview)) = stmt
                .query_row([candidate_id], map_semantic_row)
                .optional()?
            {
                let embedding = decode_vector(&embedding_bytes);
                hit.semantic_score = cosine_similarity(&query_vec, &embedding) as f64;
                hit.heuristic_score = (phrase_overlap_score(query, &content_preview) * 0.7
                    + named_entityish_boost(query, &content_preview) * 0.3)
                    as f64;
                hit.embedding_backend = match query_embedding.backend {
                    EmbeddingBackend::OnnxLocal => "onnx_local".to_string(),
                    EmbeddingBackend::StrongLocal => "strong_local".to_string(),
                    EmbeddingBackend::LexicalFallback => "lexical_fallback".to_string(),
                };
                hits.push(hit);
            }
        }
        hits.sort_by(|a, b| {
            let score_a = a.semantic_score * 0.75 + a.heuristic_score * 0.25;
            let score_b = b.semantic_score * 0.75 + b.heuristic_score * 0.25;
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn taxonomy(&self) -> Result<serde_json::Value> {
        let mut stmt = self.conn.prepare("SELECT wing, room, COUNT(*) FROM drawers WHERE active = 1 GROUP BY wing, room ORDER BY wing, room")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut taxonomy: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        for row in rows {
            let (wing, room, count) = row?;
            taxonomy.entry(wing).or_default().insert(room, count);
        }
        Ok(serde_json::json!({"taxonomy": taxonomy}))
    }

    pub fn check_duplicate(&self, content: &str, threshold: f64) -> Result<serde_json::Value> {
        let hits = self.search_hybrid(content, None, None, 5)?;
        let matches: Vec<_> = hits
            .into_iter()
            .filter(|h| {
                let combined = (h.semantic_score * 0.5)
                    + (h.lexical_score * 0.25)
                    + (h.heuristic_score * 0.25);
                combined >= threshold
            })
            .collect();
        Ok(serde_json::json!({"is_duplicate": !matches.is_empty(), "matches": matches}))
    }

    pub fn delete_drawer(&self, drawer_id: &str) -> Result<bool> {
        let changed = self
            .conn
            .execute("DELETE FROM drawers WHERE id = ?1", [drawer_id])?;
        self.conn
            .execute("DELETE FROM vectors WHERE drawer_id = ?1", [drawer_id])?;
        Ok(changed > 0)
    }

    pub fn find_compressed_for_raw(&self, raw_drawer_id: &str) -> Result<Option<Drawer>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight
             FROM drawers
             WHERE active = 1 AND drawer_type = 'compressed' AND id LIKE ?1
             ORDER BY filed_at DESC LIMIT 1"
        )?;
        stmt.query_row([format!("aaak_{}%", raw_drawer_id)], map_drawer_full)
            .optional()
            .map_err(Into::into)
    }

    fn resolve_artifact_hits(&self, hits: Vec<SearchHit>) -> Result<Vec<SearchHit>> {
        let mut resolved = Vec::new();
        for hit in hits {
            if is_derived_type(&hit.drawer_type) {
                if let Some(parent) = self.find_parent_raw_hit(&hit)? {
                    let mut parent_hit = parent;
                    parent_hit.fused_score += hit.fused_score * 0.6;
                    parent_hit.lexical_score = parent_hit.lexical_score.max(hit.lexical_score);
                    parent_hit.semantic_score = parent_hit.semantic_score.max(hit.semantic_score);
                    parent_hit.heuristic_score =
                        parent_hit.heuristic_score.max(hit.heuristic_score);
                    parent_hit.parent_drawer_id = Some(hit.id.clone());
                    resolved.push(parent_hit);
                    continue;
                }
            }
            resolved.push(hit);
        }
        let mut deduped: HashMap<String, SearchHit> = HashMap::new();
        for hit in resolved {
            let key = hit.id.clone();
            deduped
                .entry(key)
                .and_modify(|existing| {
                    if hit.fused_score > existing.fused_score {
                        *existing = hit.clone();
                    }
                })
                .or_insert(hit);
        }
        let mut out: Vec<_> = deduped.into_values().collect();
        out.sort_by(|a, b| {
            b.fused_score
                .partial_cmp(&a.fused_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    fn find_parent_raw_hit(&self, artifact: &SearchHit) -> Result<Option<SearchHit>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, wing, room, source_file, chunk_index, added_by, filed_at, substr(content, 1, 300), drawer_type
             FROM drawers
             WHERE active = 1 AND source_file = ?1 AND drawer_type = 'drawer'
             ORDER BY chunk_index ASC LIMIT 1"
        )?;
        stmt.query_row(params![artifact.source_file], |row| {
            Ok(SearchHit {
                id: row.get(0)?,
                wing: row.get(1)?,
                room: row.get(2)?,
                source_file: row.get(3)?,
                chunk_index: row.get(4)?,
                added_by: row.get(5)?,
                filed_at: row.get(6)?,
                snippet: row.get(7)?,
                lexical_score: 0.0,
                semantic_score: 0.0,
                heuristic_score: 0.0,
                fused_score: 0.0,
                drawer_type: row.get(8)?,
                embedding_backend: artifact.embedding_backend.clone(),
                parent_drawer_id: Some(artifact.id.clone()),
            })
        })
        .optional()
        .map_err(Into::into)
    }
}

fn is_derived_type(drawer_type: &str) -> bool {
    !matches!(drawer_type, "drawer" | "diary_entry")
}

fn refresh_source_in_tx(tx: &Transaction<'_>, plan: SourceRefreshPlan<'_>) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    let mut inserted = 0usize;
    for input in &plan.drawers {
        let changed = tx.execute(
            "INSERT OR REPLACE INTO drawers(id, wing, room, source_file, chunk_index, added_by, filed_at, content, hall, date, drawer_type, source_hash, active, importance, emotional_weight, weight)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?14, ?15)",
            params![input.id, input.wing, input.room, input.source_file, input.chunk_index, input.added_by, now, input.content, input.hall, input.date, input.drawer_type, input.source_hash, input.importance, input.emotional_weight, input.weight],
        )?;
        let embedding = encode_vector(&embed_text(input.content).vector);
        tx.execute(
            "INSERT OR REPLACE INTO vectors(drawer_id, embedding) VALUES (?1, ?2)",
            params![input.id, embedding],
        )?;
        if changed > 0 {
            inserted += 1;
        }
    }
    tx.execute(
        "UPDATE drawers SET active = 0 WHERE source_file = ?1 AND source_hash IS NOT ?2",
        params![plan.source_file, plan.source_hash],
    )?;
    tx.execute(
        "INSERT INTO source_revisions(source_file, source_hash, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(source_file) DO UPDATE SET source_hash=excluded.source_hash, updated_at=excluded.updated_at",
        params![plan.source_file, plan.source_hash, now],
    )?;
    Ok(inserted)
}

fn map_drawer_full(row: &rusqlite::Row<'_>) -> rusqlite::Result<Drawer> {
    Ok(Drawer {
        id: row.get(0)?,
        wing: row.get(1)?,
        room: row.get(2)?,
        source_file: row.get(3)?,
        chunk_index: row.get(4)?,
        added_by: row.get(5)?,
        filed_at: row.get(6)?,
        content: row.get(7)?,
        hall: row.get(8)?,
        date: row.get(9)?,
        drawer_type: row.get(10)?,
        source_hash: row.get(11)?,
        active: row.get::<_, i64>(12)? != 0,
        importance: row.get(13)?,
        emotional_weight: row.get(14)?,
        weight: row.get(15)?,
    })
}

fn map_semantic_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(SearchHit, Vec<u8>, String)> {
    Ok((
        SearchHit {
            id: row.get(0)?,
            wing: row.get(1)?,
            room: row.get(2)?,
            source_file: row.get(3)?,
            chunk_index: row.get(4)?,
            added_by: row.get(5)?,
            filed_at: row.get(6)?,
            snippet: row.get(7)?,
            lexical_score: 0.0,
            semantic_score: 0.0,
            heuristic_score: 0.0,
            fused_score: 0.0,
            drawer_type: row.get(8)?,
            embedding_backend: String::new(),
            parent_drawer_id: None,
        },
        row.get(9)?,
        row.get(7)?,
    ))
}
