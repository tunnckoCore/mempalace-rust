use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct KgFact {
    pub direction: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: f64,
    pub source_closet: Option<String>,
    pub current: bool,
}

pub struct KnowledgeGraph {
    conn: Connection,
}

impl KnowledgeGraph {
    pub fn open(base_dir: &Path) -> Result<Self> {
        fs::create_dir_all(base_dir)?;
        let conn = Connection::open(base_dir.join("knowledge_graph.sqlite3"))?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT DEFAULT 'unknown',
                properties TEXT DEFAULT '{}',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS triples (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                valid_from TEXT,
                valid_to TEXT,
                confidence REAL DEFAULT 1.0,
                source_closet TEXT,
                source_file TEXT,
                extracted_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_triples_subject ON triples(subject);
            CREATE INDEX IF NOT EXISTS idx_triples_object ON triples(object);
            CREATE INDEX IF NOT EXISTS idx_triples_predicate ON triples(predicate);
            CREATE INDEX IF NOT EXISTS idx_triples_valid ON triples(valid_from, valid_to);
            ",
        )?;
        Ok(Self { conn })
    }

    pub fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        valid_from: Option<&str>,
        valid_to: Option<&str>,
        confidence: f64,
        source_closet: Option<&str>,
        source_file: Option<&str>,
    ) -> Result<String> {
        let sub_id = entity_id(subject);
        let obj_id = entity_id(object);
        let pred = predicate.to_lowercase().replace(' ', "_");
        self.conn.execute(
            "INSERT OR IGNORE INTO entities (id, name) VALUES (?1, ?2)",
            params![sub_id, subject],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO entities (id, name) VALUES (?1, ?2)",
            params![obj_id, object],
        )?;
        if let Some(existing) = self.conn.query_row(
            "SELECT id FROM triples WHERE subject=?1 AND predicate=?2 AND object=?3 AND valid_to IS NULL LIMIT 1",
            params![sub_id, pred, obj_id],
            |row| row.get::<_, String>(0),
        ).optional()? {
            return Ok(existing);
        }
        let triple_id = format!(
            "t_{}_{}_{}_{}",
            sub_id,
            pred,
            obj_id,
            Utc::now().timestamp()
        );
        self.conn.execute(
            "INSERT INTO triples (id, subject, predicate, object, valid_from, valid_to, confidence, source_closet, source_file)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![triple_id, sub_id, pred, obj_id, valid_from, valid_to, confidence, source_closet, source_file],
        )?;
        Ok(triple_id)
    }

    pub fn invalidate(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        ended: Option<&str>,
    ) -> Result<()> {
        let sub_id = entity_id(subject);
        let obj_id = entity_id(object);
        let pred = predicate.to_lowercase().replace(' ', "_");
        let ended = ended
            .map(ToString::to_string)
            .unwrap_or_else(|| Utc::now().date_naive().to_string());
        self.conn.execute(
            "UPDATE triples SET valid_to=?1 WHERE subject=?2 AND predicate=?3 AND object=?4 AND valid_to IS NULL",
            params![ended, sub_id, pred, obj_id],
        )?;
        Ok(())
    }

    pub fn query_entity(
        &self,
        name: &str,
        as_of: Option<&str>,
        direction: &str,
    ) -> Result<Vec<KgFact>> {
        let eid = entity_id(name);
        let mut results = Vec::new();
        if direction == "outgoing" || direction == "both" {
            let mut sql = "SELECT t.predicate, t.valid_from, t.valid_to, t.confidence, t.source_closet, e.name FROM triples t JOIN entities e ON t.object = e.id WHERE t.subject = ?1".to_string();
            if as_of.is_some() {
                sql.push_str(" AND (t.valid_from IS NULL OR t.valid_from <= ?2) AND (t.valid_to IS NULL OR t.valid_to >= ?2)");
            }
            let mut stmt = self.conn.prepare(&sql)?;
            if let Some(as_of) = as_of {
                let rows = stmt.query_map(params![eid, as_of], |row| {
                    Ok(KgFact {
                        direction: "outgoing".to_string(),
                        subject: name.to_string(),
                        predicate: row.get(0)?,
                        object: row.get(5)?,
                        valid_from: row.get(1)?,
                        valid_to: row.get(2)?,
                        confidence: row.get(3)?,
                        source_closet: row.get(4)?,
                        current: row.get::<_, Option<String>>(2)?.is_none(),
                    })
                })?;
                results.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
            } else {
                let rows = stmt.query_map(params![eid], |row| {
                    Ok(KgFact {
                        direction: "outgoing".to_string(),
                        subject: name.to_string(),
                        predicate: row.get(0)?,
                        object: row.get(5)?,
                        valid_from: row.get(1)?,
                        valid_to: row.get(2)?,
                        confidence: row.get(3)?,
                        source_closet: row.get(4)?,
                        current: row.get::<_, Option<String>>(2)?.is_none(),
                    })
                })?;
                results.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
            }
        }
        if direction == "incoming" || direction == "both" {
            let mut sql = "SELECT t.predicate, t.valid_from, t.valid_to, t.confidence, t.source_closet, e.name FROM triples t JOIN entities e ON t.subject = e.id WHERE t.object = ?1".to_string();
            if as_of.is_some() {
                sql.push_str(" AND (t.valid_from IS NULL OR t.valid_from <= ?2) AND (t.valid_to IS NULL OR t.valid_to >= ?2)");
            }
            let mut stmt = self.conn.prepare(&sql)?;
            if let Some(as_of) = as_of {
                let rows = stmt.query_map(params![eid, as_of], |row| {
                    Ok(KgFact {
                        direction: "incoming".to_string(),
                        subject: row.get(5)?,
                        predicate: row.get(0)?,
                        object: name.to_string(),
                        valid_from: row.get(1)?,
                        valid_to: row.get(2)?,
                        confidence: row.get(3)?,
                        source_closet: row.get(4)?,
                        current: row.get::<_, Option<String>>(2)?.is_none(),
                    })
                })?;
                results.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
            } else {
                let rows = stmt.query_map(params![eid], |row| {
                    Ok(KgFact {
                        direction: "incoming".to_string(),
                        subject: row.get(5)?,
                        predicate: row.get(0)?,
                        object: name.to_string(),
                        valid_from: row.get(1)?,
                        valid_to: row.get(2)?,
                        confidence: row.get(3)?,
                        source_closet: row.get(4)?,
                        current: row.get::<_, Option<String>>(2)?.is_none(),
                    })
                })?;
                results.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
            }
        }
        Ok(results)
    }

    pub fn timeline(&self, entity_name: Option<&str>) -> Result<Vec<KgFact>> {
        let (sql, params_any): (String, Vec<String>) = if let Some(entity_name) = entity_name {
            let eid = entity_id(entity_name);
            (
                "SELECT s.name, t.predicate, o.name, t.valid_from, t.valid_to, t.confidence, t.source_closet FROM triples t JOIN entities s ON t.subject = s.id JOIN entities o ON t.object = o.id WHERE (t.subject = ?1 OR t.object = ?1) ORDER BY COALESCE(t.valid_from, '') ASC".to_string(),
                vec![eid],
            )
        } else {
            (
                "SELECT s.name, t.predicate, o.name, t.valid_from, t.valid_to, t.confidence, t.source_closet FROM triples t JOIN entities s ON t.subject = s.id JOIN entities o ON t.object = o.id ORDER BY COALESCE(t.valid_from, '') ASC LIMIT 100".to_string(),
                vec![],
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        if params_any.is_empty() {
            let rows = stmt.query_map([], |row| {
                Ok(KgFact {
                    direction: "timeline".to_string(),
                    subject: row.get(0)?,
                    predicate: row.get(1)?,
                    object: row.get(2)?,
                    valid_from: row.get(3)?,
                    valid_to: row.get(4)?,
                    confidence: row.get(5)?,
                    source_closet: row.get(6)?,
                    current: row.get::<_, Option<String>>(4)?.is_none(),
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        } else {
            let rows = stmt.query_map(params![params_any[0]], |row| {
                Ok(KgFact {
                    direction: "timeline".to_string(),
                    subject: row.get(0)?,
                    predicate: row.get(1)?,
                    object: row.get(2)?,
                    valid_from: row.get(3)?,
                    valid_to: row.get(4)?,
                    confidence: row.get(5)?,
                    source_closet: row.get(6)?,
                    current: row.get::<_, Option<String>>(4)?.is_none(),
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        }
    }

    pub fn stats(&self) -> Result<serde_json::Value> {
        let entities: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
        let triples: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM triples", [], |r| r.get(0))?;
        let current: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM triples WHERE valid_to IS NULL",
            [],
            |r| r.get(0),
        )?;
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT predicate FROM triples ORDER BY predicate")?;
        let rels = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(
            serde_json::json!({"entities": entities, "triples": triples, "current_facts": current, "expired_facts": triples - current, "relationship_types": rels}),
        )
    }
}

fn entity_id(name: &str) -> String {
    name.to_lowercase().replace(' ', "_").replace('"', "")
}
