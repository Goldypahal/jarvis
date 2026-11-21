//! SQLite storage (SRS FR-53, router quota ledger): one file,
//! `~/.oblivion/oblivion.db`, holding saved sessions, per-day usage of
//! every model so free-tier daily limits survive restarts, and long-term memory.

pub mod embedding;
pub mod memory;
pub mod migrations;
pub mod schedule;
pub mod tools;

pub use embedding::{EmbedFuture, Embedder, MockEmbedder, OllamaEmbedder};
pub use memory::{Memory, MemoryFilter, NewMemory};
pub use schedule::{NewScheduledTask, ScheduledTask, UpdateScheduledTask};
pub use tools::MemoryTools;

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use oblivion_core::session::{Session, SessionInfo, SessionStore};
use oblivion_providers::{Role, Usage, UsageLedger};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

static INIT_SQLITE_VEC: std::sync::Once = std::sync::Once::new();

/// Registers sqlite-vec extension globally for all SQLite connections in this process.
#[allow(clippy::missing_transmute_annotations)]
pub fn ensure_sqlite_vec_registered() {
    INIT_SQLITE_VEC.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

pub struct Store {
    conn: Mutex<Connection>,
    embedder: RwLock<Option<Arc<dyn Embedder>>>,
}

/// Today's usage of one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRow {
    pub target: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Free tiers reset on UTC days.
fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn sanitize_fts5_query(query: &str) -> String {
    let mut terms = Vec::new();
    for raw_word in query.split_whitespace() {
        let word = raw_word.trim();
        if word == "OR" {
            if !terms.is_empty() && terms.last() != Some(&"OR".to_string()) {
                terms.push("OR".to_string());
            }
            continue;
        }
        for part in word.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
            let clean = part.trim();
            if !clean.is_empty() && clean != "OR" {
                terms.push(format!("{clean}*"));
            }
        }
    }
    while terms.last() == Some(&"OR".to_string()) {
        terms.pop();
    }
    while terms.first() == Some(&"OR".to_string()) {
        terms.remove(0);
    }
    terms.join(" ")
}

fn sanitize_fts5_or_query(query: &str) -> String {
    const STOPWORDS: &[&str] = &[
        "a",
        "about",
        "above",
        "after",
        "again",
        "against",
        "all",
        "am",
        "an",
        "and",
        "any",
        "are",
        "aren't",
        "as",
        "at",
        "be",
        "because",
        "been",
        "before",
        "being",
        "below",
        "between",
        "both",
        "but",
        "by",
        "can",
        "can't",
        "cannot",
        "could",
        "couldn't",
        "did",
        "didn't",
        "do",
        "does",
        "doesn't",
        "doing",
        "don't",
        "down",
        "during",
        "each",
        "few",
        "for",
        "from",
        "further",
        "had",
        "hadn't",
        "has",
        "hasn't",
        "have",
        "haven't",
        "having",
        "he",
        "her",
        "here",
        "hers",
        "herself",
        "him",
        "himself",
        "his",
        "how",
        "i",
        "if",
        "in",
        "into",
        "is",
        "isn't",
        "it",
        "it's",
        "its",
        "itself",
        "let's",
        "me",
        "more",
        "most",
        "mustn't",
        "my",
        "myself",
        "no",
        "nor",
        "not",
        "of",
        "off",
        "on",
        "once",
        "only",
        "or",
        "other",
        "ought",
        "our",
        "ours",
        "ourselves",
        "out",
        "over",
        "own",
        "same",
        "she",
        "should",
        "shouldn't",
        "so",
        "some",
        "such",
        "than",
        "that",
        "the",
        "their",
        "theirs",
        "them",
        "themselves",
        "then",
        "there",
        "these",
        "they",
        "this",
        "those",
        "through",
        "to",
        "too",
        "under",
        "until",
        "up",
        "very",
        "was",
        "wasn't",
        "we",
        "were",
        "weren't",
        "what",
        "when",
        "where",
        "which",
        "while",
        "who",
        "whom",
        "why",
        "with",
        "won't",
        "would",
        "wouldn't",
        "you",
        "your",
        "yours",
        "yourself",
        "yourselves",
    ];

    let mut terms = Vec::new();
    for word in query.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let w = word.trim().to_lowercase();
        if w.len() > 1 && !STOPWORDS.contains(&w.as_str()) {
            terms.push(format!("{w}*"));
        }
    }
    if terms.is_empty() {
        return String::new();
    }
    terms.join(" OR ")
}

fn row_to_memory(r: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        mem_type: r.get(1)?,
        text: r.get(2)?,
        source_session: r.get(3)?,
        project: r.get(4)?,
        created: r.get(5)?,
        last_used: r.get(6)?,
        importance: r.get::<_, i64>(7)? as u32,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        ensure_sqlite_vec_registered();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut conn =
            Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 3000)?;
        migrations::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            embedder: RwLock::new(None),
        })
    }

    pub fn in_memory() -> Result<Self> {
        ensure_sqlite_vec_registered();
        let mut conn = Connection::open_in_memory()?;
        migrations::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            embedder: RwLock::new(None),
        })
    }

    pub fn load_session(&self, id: &str) -> Result<Session> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(String, String, String)> = conn
            .query_row(
                "SELECT created, workspace, messages FROM sessions WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (created, workspace, messages) = row.with_context(|| format!("no session {id}"))?;
        Ok(Session {
            id: id.to_string(),
            created,
            workspace: workspace.into(),
            messages: serde_json::from_str(&messages)?,
        })
    }

    /// Most recent first.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionInfo>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, created, updated, workspace, title, json_array_length(messages)
             FROM sessions ORDER BY updated DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(SessionInfo {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    workspace: r.get::<_, String>(3)?.into(),
                    first_prompt: r.get(4)?,
                    updated: r.get(2).ok(),
                    message_count: r.get::<_, Option<i64>>(5)?.map(|n| n.max(0) as usize),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Search sessions by title, id, workspace, or messages content.
    pub fn search_sessions(&self, query: &str, limit: usize) -> Result<Vec<SessionInfo>> {
        let conn = self.conn.lock().unwrap();
        let q = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "SELECT id, created, updated, workspace, title, json_array_length(messages)
             FROM sessions
             WHERE id LIKE ?1 OR title LIKE ?1 OR workspace LIKE ?1 OR messages LIKE ?1
             ORDER BY updated DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![q, limit as i64], |r| {
                Ok(SessionInfo {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    workspace: r.get::<_, String>(3)?.into(),
                    first_prompt: r.get(4)?,
                    updated: r.get(2).ok(),
                    message_count: r.get::<_, Option<i64>>(5)?.map(|n| n.max(0) as usize),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Rename an existing session.
    pub fn rename_session(&self, id: &str, title: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let affected = conn.execute(
            "UPDATE sessions SET title = ?1, updated = ?2 WHERE id = ?3",
            params![title.trim(), now, id],
        )?;
        Ok(affected > 0)
    }

    /// Delete an existing session.
    pub fn delete_session(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let affected = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// Update workspace for an existing session.
    pub fn update_session_workspace(&self, id: &str, workspace: &Path) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Local::now().to_rfc3339();
        let affected = conn.execute(
            "UPDATE sessions SET workspace = ?1, updated = ?2 WHERE id = ?3",
            params![workspace.to_string_lossy(), now, id],
        )?;
        Ok(affected > 0)
    }

    /// Copy Phase 0 JSON sessions into the database (once; existing ids are kept).
    pub fn import_json_dir(&self, dir: &Path) -> Result<usize> {
        let mut n = 0;
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Ok(0);
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(s) = serde_json::from_str::<Session>(&text) else {
                continue;
            };
            let exists: bool = self.conn.lock().unwrap().query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                params![s.id],
                |r| r.get(0),
            )?;
            if !exists {
                self.save(&s)?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn usage_today(&self) -> Result<Vec<UsageRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT target, requests, prompt_tokens, completion_tokens FROM usage WHERE day = ?1 ORDER BY requests DESC",
        )?;
        let rows = stmt
            .query_map(params![today()], |r| {
                Ok(UsageRow {
                    target: r.get(0)?,
                    requests: r.get::<_, i64>(1)? as u64,
                    prompt_tokens: r.get::<_, i64>(2)? as u64,
                    completion_tokens: r.get::<_, i64>(3)? as u64,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Add a new long-term memory.
    pub fn add_memory(&self, mem: &NewMemory) -> Result<Memory> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let importance = mem.importance.clamp(1, 5) as i64;
        conn.execute(
            "INSERT INTO memories (mem_type, text, source_session, project, created, last_used, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![
                mem.mem_type,
                mem.text,
                mem.source_session,
                mem.project,
                now,
                importance
            ],
        )?;
        let id = conn.last_insert_rowid();
        Ok(Memory {
            id,
            mem_type: mem.mem_type.clone(),
            text: mem.text.clone(),
            source_session: mem.source_session.clone(),
            project: mem.project.clone(),
            created: now.clone(),
            last_used: now,
            importance: mem.importance.clamp(1, 5),
        })
    }

    /// Get a memory by ID.
    pub fn get_memory(&self, id: i64) -> Result<Option<Memory>> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT id, mem_type, text, source_session, project, created, last_used, importance
                 FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?;
        Ok(row)
    }

    /// Update a memory's text, type, or importance.
    pub fn update_memory(
        &self,
        id: i64,
        text: Option<&str>,
        mem_type: Option<&str>,
        importance: Option<u32>,
    ) -> Result<Option<Memory>> {
        let conn = self.conn.lock().unwrap();
        let existing: Option<Memory> = conn
            .query_row(
                "SELECT id, mem_type, text, source_session, project, created, last_used, importance
                 FROM memories WHERE id = ?1",
                params![id],
                row_to_memory,
            )
            .optional()?;

        let Some(mut mem) = existing else {
            return Ok(None);
        };

        if let Some(t) = text {
            mem.text = t.to_string();
        }
        if let Some(mt) = mem_type {
            mem.mem_type = mt.to_string();
        }
        if let Some(imp) = importance {
            mem.importance = imp.clamp(1, 5);
        }

        conn.execute(
            "UPDATE memories SET mem_type = ?1, text = ?2, importance = ?3 WHERE id = ?4",
            params![mem.mem_type, mem.text, mem.importance as i64, id],
        )?;

        Ok(Some(mem))
    }

    /// Update the last_used timestamp to now.
    pub fn touch_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memories SET last_used = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Delete a memory by ID. Returns true if a record was removed.
    pub fn delete_memory(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM vec_memories WHERE memory_id = ?1", params![id]);
        let affected = conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// Count total memories matching an optional filter.
    pub fn count_memories(&self, filter: Option<&MemoryFilter>) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT count(*) FROM memories WHERE 1=1".to_string();
        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(f) = filter {
            if let Some(ref t) = f.mem_type {
                sql.push_str(" AND mem_type = ?");
                dyn_params.push(Box::new(t.clone()));
            }
            if let Some(ref s) = f.source_session {
                sql.push_str(" AND source_session = ?");
                dyn_params.push(Box::new(s.clone()));
            }
            if let Some(ref proj) = f.project {
                if f.include_global {
                    sql.push_str(" AND (project = ? OR project IS NULL)");
                    dyn_params.push(Box::new(proj.clone()));
                } else {
                    sql.push_str(" AND project = ?");
                    dyn_params.push(Box::new(proj.clone()));
                }
            }
            if let Some(min_imp) = f.min_importance {
                sql.push_str(" AND importance >= ?");
                dyn_params.push(Box::new(min_imp as i64));
            }
        }

        let param_refs: Vec<&dyn rusqlite::ToSql> = dyn_params.iter().map(|b| b.as_ref()).collect();
        let count: i64 = conn.query_row(&sql, param_refs.as_slice(), |r| r.get(0))?;
        Ok(count as u64)
    }

    /// List memories ordered by last_used DESC, importance DESC.
    pub fn list_memories(
        &self,
        filter: Option<&MemoryFilter>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT id, mem_type, text, source_session, project, created, last_used, importance FROM memories WHERE 1=1".to_string();
        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(f) = filter {
            if let Some(ref t) = f.mem_type {
                sql.push_str(" AND mem_type = ?");
                dyn_params.push(Box::new(t.clone()));
            }
            if let Some(ref s) = f.source_session {
                sql.push_str(" AND source_session = ?");
                dyn_params.push(Box::new(s.clone()));
            }
            if let Some(ref proj) = f.project {
                if f.include_global {
                    sql.push_str(" AND (project = ? OR project IS NULL)");
                    dyn_params.push(Box::new(proj.clone()));
                } else {
                    sql.push_str(" AND project = ?");
                    dyn_params.push(Box::new(proj.clone()));
                }
            }
            if let Some(min_imp) = f.min_importance {
                sql.push_str(" AND importance >= ?");
                dyn_params.push(Box::new(min_imp as i64));
            }
        }

        sql.push_str(" ORDER BY last_used DESC, importance DESC, id DESC LIMIT ?");
        dyn_params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = dyn_params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), row_to_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Search memories using full-text search (FTS5) ranked by relevance (bm25).
    pub fn search_memories(
        &self,
        query_str: &str,
        filter: Option<&MemoryFilter>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let sanitized = sanitize_fts5_query(query_str);
        if sanitized.is_empty() {
            return self.list_memories(filter, limit);
        }

        let conn = self.conn.lock().unwrap();
        let mut sql = "SELECT m.id, m.mem_type, m.text, m.source_session, m.project, m.created, m.last_used, m.importance
                       FROM memories m
                       JOIN memories_fts f ON m.id = f.rowid
                       WHERE memories_fts MATCH ?".to_string();

        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        dyn_params.push(Box::new(sanitized));

        if let Some(f) = filter {
            if let Some(ref t) = f.mem_type {
                sql.push_str(" AND m.mem_type = ?");
                dyn_params.push(Box::new(t.clone()));
            }
            if let Some(ref s) = f.source_session {
                sql.push_str(" AND m.source_session = ?");
                dyn_params.push(Box::new(s.clone()));
            }
            if let Some(ref proj) = f.project {
                if f.include_global {
                    sql.push_str(" AND (m.project = ? OR m.project IS NULL)");
                    dyn_params.push(Box::new(proj.clone()));
                } else {
                    sql.push_str(" AND m.project = ?");
                    dyn_params.push(Box::new(proj.clone()));
                }
            }
            if let Some(min_imp) = f.min_importance {
                sql.push_str(" AND m.importance >= ?");
                dyn_params.push(Box::new(min_imp as i64));
            }
        }

        sql.push_str(
            " ORDER BY bm25(memories_fts) ASC, m.importance DESC, m.last_used DESC LIMIT ?",
        );
        dyn_params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = dyn_params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), row_to_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Attach an embedder to this store for semantic vector search.
    pub fn set_embedder(&self, embedder: Arc<dyn Embedder>) {
        *self.embedder.write().unwrap() = Some(embedder);
    }

    /// Retrieve the attached embedder, if any.
    pub fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.read().unwrap().clone()
    }

    /// Ensure the sqlite-vec virtual table `vec_memories` exists with the given dimension.
    pub fn ensure_vector_table(&self, dimensions: usize) -> Result<()> {
        ensure_sqlite_vec_registered();
        let conn = self.conn.lock().unwrap();
        let sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(memory_id integer primary key, embedding float[{dimensions}]);"
        );
        conn.execute(&sql, [])?;
        Ok(())
    }

    /// Save an embedding vector for a memory.
    pub fn save_memory_embedding(&self, memory_id: i64, model: &str, vector: &[f32]) -> Result<()> {
        if vector.is_empty() {
            return Ok(());
        }
        self.ensure_vector_table(vector.len())?;
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let bytes = vector_to_bytes(vector);

        conn.execute(
            "INSERT INTO memory_embeddings (memory_id, model, dimensions, embedding, created)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(memory_id) DO UPDATE SET model = excluded.model,
                 dimensions = excluded.dimensions,
                 embedding = excluded.embedding,
                 created = excluded.created",
            params![memory_id, model, vector.len() as i64, bytes, now],
        )?;

        let _ = conn.execute(
            "DELETE FROM vec_memories WHERE memory_id = ?1",
            params![memory_id],
        );
        conn.execute(
            "INSERT INTO vec_memories (memory_id, embedding) VALUES (?1, ?2)",
            params![memory_id, bytes],
        )?;

        Ok(())
    }

    /// Get stored vector embedding for a memory ID.
    pub fn get_memory_embedding(&self, memory_id: i64) -> Result<Option<Vec<f32>>> {
        let conn = self.conn.lock().unwrap();
        let bytes: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM memory_embeddings WHERE memory_id = ?1",
                params![memory_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(bytes.map(|b| bytes_to_vector(&b)))
    }

    /// Search memories using vector cosine / distance similarity (sqlite-vec).
    pub fn search_memories_vector(
        &self,
        embedding: &[f32],
        filter: Option<&MemoryFilter>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if embedding.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='vec_memories'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        if !exists {
            return Ok(Vec::new());
        }

        let bytes = vector_to_bytes(embedding);
        let mut sql = "SELECT m.id, m.mem_type, m.text, m.source_session, m.project, m.created, m.last_used, m.importance
                       FROM vec_memories v
                       JOIN memories m ON v.memory_id = m.id
                       WHERE v.embedding MATCH ?1 AND k = ?2".to_string();

        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        dyn_params.push(Box::new(bytes));
        let fetch_k = (limit * 3).max(10);
        dyn_params.push(Box::new(fetch_k as i64));

        if let Some(f) = filter {
            if let Some(ref t) = f.mem_type {
                sql.push_str(" AND m.mem_type = ?");
                dyn_params.push(Box::new(t.clone()));
            }
            if let Some(ref s) = f.source_session {
                sql.push_str(" AND m.source_session = ?");
                dyn_params.push(Box::new(s.clone()));
            }
            if let Some(ref proj) = f.project {
                if f.include_global {
                    sql.push_str(" AND (m.project = ? OR m.project IS NULL)");
                    dyn_params.push(Box::new(proj.clone()));
                } else {
                    sql.push_str(" AND m.project = ?");
                    dyn_params.push(Box::new(proj.clone()));
                }
            }
            if let Some(min_imp) = f.min_importance {
                sql.push_str(" AND m.importance >= ?");
                dyn_params.push(Box::new(min_imp as i64));
            }
        }

        sql.push_str(" ORDER BY v.distance ASC, m.importance DESC LIMIT ?");
        dyn_params.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = dyn_params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(param_refs.as_slice(), row_to_memory)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Hybrid search: tries vector similarity first when embedding is provided; falls back to FTS5.
    pub fn search_memories_hybrid(
        &self,
        query_str: &str,
        embedding: Option<&[f32]>,
        filter: Option<&MemoryFilter>,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        if let Some(emb) = embedding {
            if let Ok(vec_res) = self.search_memories_vector(emb, filter, limit) {
                if !vec_res.is_empty() {
                    return Ok(vec_res);
                }
            }
        }
        self.search_memories(query_str, filter, limit)
    }

    // ---------- scheduled tasks ----------

    pub fn add_scheduled_task(&self, task: &NewScheduledTask) -> Result<ScheduledTask> {
        let conn = self.conn.lock().unwrap();
        schedule::db_add_task(&conn, task)
    }

    pub fn get_scheduled_task(&self, id: &str) -> Result<Option<ScheduledTask>> {
        let conn = self.conn.lock().unwrap();
        schedule::db_get_task(&conn, id)
    }

    pub fn list_scheduled_tasks(&self, enabled_only: bool) -> Result<Vec<ScheduledTask>> {
        let conn = self.conn.lock().unwrap();
        schedule::db_list_tasks(&conn, enabled_only)
    }

    pub fn due_scheduled_tasks(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<ScheduledTask>> {
        let conn = self.conn.lock().unwrap();
        schedule::db_due_tasks(&conn, now)
    }

    pub fn update_scheduled_task(
        &self,
        id: &str,
        updates: &UpdateScheduledTask,
    ) -> Result<ScheduledTask> {
        let conn = self.conn.lock().unwrap();
        schedule::db_update_task(&conn, id, updates)
    }

    pub fn delete_scheduled_task(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        schedule::db_delete_task(&conn, id)
    }

    pub fn record_task_run(
        &self,
        id: &str,
        session_id: &str,
        outcome: &str,
        error: Option<&str>,
        next_run: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        schedule::db_record_task_run(&conn, id, session_id, outcome, error, next_run)
    }
}

fn vector_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_vector(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect()
}

impl SessionStore for Store {
    fn save(&self, s: &Session) -> Result<()> {
        let title: String = s
            .messages
            .iter()
            .find(|m| m.role == Role::User)
            .and_then(|m| m.content.as_deref())
            .unwrap_or("")
            .chars()
            .take(70)
            .collect();
        let messages = serde_json::to_string(&s.messages)?;
        self.conn.lock().unwrap().execute(
            "INSERT INTO sessions (id, created, updated, workspace, title, messages)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET updated = excluded.updated, title = excluded.title,
                                           messages = excluded.messages",
            params![
                s.id,
                s.created,
                chrono::Local::now().to_rfc3339(),
                s.workspace.to_string_lossy(),
                title,
                messages
            ],
        )?;
        Ok(())
    }
}

impl UsageLedger for Store {
    fn requests_today(&self, label: &str) -> u64 {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT requests FROM usage WHERE day = ?1 AND target = ?2",
                params![today(), label],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(0) as u64
    }

    fn record(&self, label: &str, usage: Option<&Usage>) {
        let (p, c) = usage.map_or((0, 0), |u| {
            (u.prompt_tokens as i64, u.completion_tokens as i64)
        });
        // A failed write must not stop the agent; usage is advisory.
        let _ = self.conn.lock().unwrap().execute(
            "INSERT INTO usage (day, target, requests, prompt_tokens, completion_tokens)
             VALUES (?1, ?2, 1, ?3, ?4)
             ON CONFLICT(day, target) DO UPDATE SET requests = requests + 1,
                 prompt_tokens = prompt_tokens + excluded.prompt_tokens,
                 completion_tokens = completion_tokens + excluded.completion_tokens",
            params![today(), label, p, c],
        );
    }
}

impl oblivion_core::MemoryRecall for Store {
    fn recall_for_prompt<'a>(
        &'a self,
        prompt: &'a str,
        project: &'a str,
        budget_chars: usize,
    ) -> oblivion_core::RecallFuture<'a> {
        let prompt = prompt.to_string();
        let project = project.to_string();
        Box::pin(async move {
            let filter = MemoryFilter {
                project: Some(project.clone()),
                include_global: true,
                ..Default::default()
            };

            // 1. Try vector similarity search if embedder is configured
            let mut memories = if let Some(embedder) = self.embedder() {
                if let Ok(vec) = embedder.embed(&prompt).await {
                    self.search_memories_vector(&vec, Some(&filter), 10)
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // 2. If vector search returned nothing or embedder offline, fall back to FTS5
            if memories.is_empty() {
                let or_query = sanitize_fts5_or_query(&prompt);
                if !or_query.is_empty() {
                    memories = self
                        .search_memories(&or_query, Some(&filter), 10)
                        .unwrap_or_default();
                }
            }

            // 3. Fallback to core preferences if under 3 memories
            if memories.len() < 3 {
                let high_imp_filter = MemoryFilter {
                    project: Some(project),
                    include_global: true,
                    min_importance: Some(4),
                    ..Default::default()
                };
                if let Ok(high_imp) = self.list_memories(Some(&high_imp_filter), 5) {
                    for m in high_imp {
                        if !memories.iter().any(|existing| existing.id == m.id) {
                            memories.push(m);
                        }
                    }
                }
            }

            let mut out = Vec::new();
            let mut total_chars = 0;

            for m in memories {
                let line = format!("[{}] {}", m.mem_type, m.text);
                if total_chars + line.len() > budget_chars {
                    break;
                }
                total_chars += line.len() + 1;
                out.push(line);
                let _ = self.touch_memory(m.id);
            }
            out
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oblivion_providers::Message;

    #[test]
    fn sqlite_vec_extension_works() {
        ensure_sqlite_vec_registered();
        let conn = Connection::open_in_memory().unwrap();
        let version: String = conn
            .query_row("SELECT vec_version()", [], |r| r.get(0))
            .unwrap();
        assert!(!version.is_empty());

        // Test creating a vec0 virtual table
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_test USING vec0(memory_id integer primary key, embedding float[4]);",
            [],
        )
        .unwrap();

        // Test vec_distance_cosine scalar function
        let dist: f32 = conn
            .query_row(
                "SELECT vec_distance_cosine(?1, ?2)",
                params![
                    to_bytes(&[1.0, 0.0, 0.0, 0.0]),
                    to_bytes(&[1.0, 0.0, 0.0, 0.0])
                ],
                |r| r.get(0),
            )
            .unwrap();
        assert!(dist.abs() < 1e-5);

        // Convert f32 slice to raw bytes
        fn to_bytes(v: &[f32]) -> Vec<u8> {
            v.iter().flat_map(|f| f.to_le_bytes()).collect()
        }

        let v1 = [1.0f32, 0.0, 0.0, 0.0];
        let v2 = [0.0f32, 1.0, 0.0, 0.0];
        let v3 = [0.9f32, 0.1, 0.0, 0.0];

        conn.execute(
            "INSERT INTO vec_test(memory_id, embedding) VALUES (?1, ?2)",
            params![101i64, to_bytes(&v1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vec_test(memory_id, embedding) VALUES (?1, ?2)",
            params![102i64, to_bytes(&v2)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO vec_test(memory_id, embedding) VALUES (?1, ?2)",
            params![103i64, to_bytes(&v3)],
        )
        .unwrap();

        // Query nearest to v1: v1 (dist 0.0) and v3 (very close) should be first
        let query_v = to_bytes(&[1.0f32, 0.05, 0.0, 0.0]);
        let mut stmt = conn
            .prepare("SELECT memory_id, distance FROM vec_test WHERE embedding MATCH ?1 AND k = 2 ORDER BY distance")
            .unwrap();
        let results: Vec<(i64, f32)> = stmt
            .query_map(params![query_v], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 101); // closest
        assert_eq!(results[1].0, 103); // second closest
    }

    #[test]
    fn sessions_roundtrip_and_list() {
        let st = Store::in_memory().unwrap();
        let mut s = Session::new("/w".into());
        s.messages.push(Message::system("sys"));
        s.messages.push(Message::user("fix the login bug"));
        st.save(&s).unwrap();
        s.messages
            .push(Message::assistant(Some("done".into()), vec![]));
        st.save(&s).unwrap(); // upsert, not a second row
        let back = st.load_session(&s.id).unwrap();
        assert_eq!(back.messages.len(), 3);
        let list = st.list_sessions(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].first_prompt, "fix the login bug");
        assert_eq!(list[0].message_count, Some(3));
        assert!(st.load_session("nope").is_err());
    }

    #[test]
    fn sessions_search_rename_delete_and_workspace() {
        let st = Store::in_memory().unwrap();
        let mut s1 = Session::new("/project1".into());
        s1.id = "session-p1-001".into();
        s1.messages.push(Message::user("Build the database schema"));
        st.save(&s1).unwrap();

        let mut s2 = Session::new("/project2".into());
        s2.id = "session-p2-002".into();
        s2.messages
            .push(Message::user("Fix frontend CSS styles and modal"));
        st.save(&s2).unwrap();

        // 1. Search sessions
        let res_frontend = st.search_sessions("frontend", 10).unwrap();
        assert_eq!(res_frontend.len(), 1);
        assert_eq!(res_frontend[0].id, s2.id);

        let res_p1 = st.search_sessions("project1", 10).unwrap();
        assert_eq!(res_p1.len(), 1);
        assert_eq!(res_p1[0].id, s1.id);

        // 2. Rename session
        assert!(st
            .rename_session(&s1.id, "Refactor SQL migrations")
            .unwrap());
        let list = st.list_sessions(10).unwrap();
        let renamed = list.iter().find(|s| s.id == s1.id).unwrap();
        assert_eq!(renamed.first_prompt, "Refactor SQL migrations");

        // 3. Update workspace (B10)
        assert!(st
            .update_session_workspace(&s2.id, Path::new("/project2-new"))
            .unwrap());
        let s2_loaded = st.load_session(&s2.id).unwrap();
        assert_eq!(
            s2_loaded.workspace,
            std::path::PathBuf::from("/project2-new")
        );

        // 4. Delete session
        assert!(st.delete_session(&s1.id).unwrap());
        assert!(!st.delete_session("nonexistent").unwrap());
        assert!(st.load_session(&s1.id).is_err());
        assert_eq!(st.list_sessions(10).unwrap().len(), 1);
    }

    #[test]
    fn usage_counts_per_target() {
        let st = Store::in_memory().unwrap();
        let u = Usage {
            prompt_tokens: 100,
            completion_tokens: 10,
        };
        st.record("groq/a", Some(&u));
        st.record("groq/a", Some(&u));
        st.record("or/b", None);
        assert_eq!(st.requests_today("groq/a"), 2);
        assert_eq!(st.requests_today("missing"), 0);
        let rows = st.usage_today().unwrap();
        assert_eq!(
            rows[0],
            UsageRow {
                target: "groq/a".into(),
                requests: 2,
                prompt_tokens: 200,
                completion_tokens: 20
            }
        );
    }

    #[test]
    fn imports_phase0_json_once() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Session::new("/w".into());
        s.messages.push(Message::user("old task"));
        s.save(d.path()).unwrap();
        let st = Store::open(&d.path().join("db.sqlite")).unwrap();
        assert_eq!(st.import_json_dir(d.path()).unwrap(), 1);
        assert_eq!(st.import_json_dir(d.path()).unwrap(), 0);
        assert_eq!(st.list_sessions(5).unwrap()[0].first_prompt, "old task");
    }

    #[test]
    fn fts5_available() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("CREATE VIRTUAL TABLE test_fts USING fts5(content);", [])
            .unwrap();
        conn.execute("INSERT INTO test_fts (content) VALUES ('hello world');", [])
            .unwrap();

        // Test 1: plain match
        let count1: i64 = conn
            .query_row(
                "SELECT count(*) FROM test_fts WHERE test_fts MATCH 'hello';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count1, 1);

        // Test 2: prefix match
        let count2: i64 = conn
            .query_row(
                "SELECT count(*) FROM test_fts WHERE test_fts MATCH 'hel*';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count2, 1);

        // Test 3: OR match
        let count3: i64 = conn
            .query_row(
                "SELECT count(*) FROM test_fts WHERE test_fts MATCH 'hel* OR wor*';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count3, 1);
    }

    #[test]
    fn memory_crud_and_touch() {
        let st = Store::in_memory().unwrap();

        // 1. Add
        let new_mem = NewMemory::new(
            "user_pref",
            "prefers dark theme and concise explanations",
            4,
        )
        .with_source_session("session-abc");
        let created = st.add_memory(&new_mem).unwrap();
        assert_eq!(created.id, 1);
        assert_eq!(created.mem_type, "user_pref");
        assert_eq!(created.text, "prefers dark theme and concise explanations");
        assert_eq!(created.source_session.as_deref(), Some("session-abc"));
        assert_eq!(created.importance, 4);

        // 2. Get
        let fetched = st.get_memory(created.id).unwrap().expect("must exist");
        assert_eq!(fetched, created);
        assert!(st.get_memory(999).unwrap().is_none());

        // 3. Update
        let updated = st
            .update_memory(
                created.id,
                Some("prefers dark theme, monospaced font"),
                None,
                Some(5),
            )
            .unwrap()
            .expect("must exist");
        assert_eq!(updated.text, "prefers dark theme, monospaced font");
        assert_eq!(updated.importance, 5);
        assert_eq!(updated.mem_type, "user_pref");

        let fetched_updated = st.get_memory(created.id).unwrap().unwrap();
        assert_eq!(fetched_updated.text, "prefers dark theme, monospaced font");
        assert_eq!(fetched_updated.importance, 5);

        // 4. Touch
        let before_touch = fetched_updated.last_used.clone();
        // Short sleep or explicit touch verify
        st.touch_memory(created.id).unwrap();
        let after_touch = st.get_memory(created.id).unwrap().unwrap();
        assert!(after_touch.last_used >= before_touch);

        // 5. Delete
        assert!(st.delete_memory(created.id).unwrap());
        assert!(!st.delete_memory(created.id).unwrap());
        assert!(st.get_memory(created.id).unwrap().is_none());
    }

    #[test]
    fn memory_filtering_and_count() {
        let st = Store::in_memory().unwrap();

        st.add_memory(&NewMemory::new("fact", "Project uses Rust 2024 edition", 3))
            .unwrap();
        st.add_memory(
            &NewMemory::new("fact", "Database uses SQLite FTS5", 5).with_source_session("s1"),
        )
        .unwrap();
        st.add_memory(
            &NewMemory::new("user_pref", "User prefers concise answers", 4)
                .with_source_session("s1"),
        )
        .unwrap();
        st.add_memory(
            &NewMemory::new("recipe", "Run cargo clippy before committing", 5)
                .with_source_session("s2"),
        )
        .unwrap();

        // Total count
        assert_eq!(st.count_memories(None).unwrap(), 4);

        // Filter by type
        let fact_filter = MemoryFilter {
            mem_type: Some("fact".into()),
            ..Default::default()
        };
        assert_eq!(st.count_memories(Some(&fact_filter)).unwrap(), 2);
        let facts = st.list_memories(Some(&fact_filter), 10).unwrap();
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().all(|m| m.mem_type == "fact"));

        // Filter by min importance
        let imp_filter = MemoryFilter {
            min_importance: Some(5),
            ..Default::default()
        };
        assert_eq!(st.count_memories(Some(&imp_filter)).unwrap(), 2);
        let high_imp = st.list_memories(Some(&imp_filter), 10).unwrap();
        assert_eq!(high_imp.len(), 2);
        assert!(high_imp.iter().all(|m| m.importance >= 5));

        // Filter by session
        let s1_filter = MemoryFilter {
            source_session: Some("s1".into()),
            ..Default::default()
        };
        assert_eq!(st.count_memories(Some(&s1_filter)).unwrap(), 2);
        let s1_list = st.list_memories(Some(&s1_filter), 10).unwrap();
        assert_eq!(s1_list.len(), 2);
        assert!(s1_list
            .iter()
            .all(|m| m.source_session.as_deref() == Some("s1")));

        // Limit works
        let limited = st.list_memories(None, 2).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn memory_fts5_search_and_ranking() {
        let st = Store::in_memory().unwrap();

        st.add_memory(&NewMemory::new(
            "recipe",
            "How to build with cargo build --workspace",
            3,
        ))
        .unwrap();
        st.add_memory(&NewMemory::new(
            "fact",
            "Oblivion agent uses rusqlite with FTS5 search",
            5,
        ))
        .unwrap();
        st.add_memory(&NewMemory::new(
            "user_pref",
            "User likes fast responses and no polite boilerplate",
            4,
        ))
        .unwrap();
        st.add_memory(&NewMemory::new(
            "fact",
            "Oblivion provider routing handles rate limits and fallbacks",
            4,
        ))
        .unwrap();

        // Search for "oblivion"
        let res = st.search_memories("oblivion", None, 10).unwrap();
        assert_eq!(res.len(), 2);
        assert!(res
            .iter()
            .all(|m| m.text.to_lowercase().contains("oblivion")));

        // Search for "fts5 search"
        let res_fts = st.search_memories("fts5 search", None, 10).unwrap();
        assert_eq!(res_fts.len(), 1);
        assert_eq!(res_fts[0].mem_type, "fact");
        assert!(res_fts[0].text.contains("FTS5"));

        // Search with punctuation and special chars (sanitized)
        let res_clean = st.search_memories("cargo:build!", None, 10).unwrap();
        assert_eq!(res_clean.len(), 1);
        assert!(res_clean[0].text.contains("cargo build"));

        // Search with filter
        let filter = MemoryFilter {
            mem_type: Some("fact".into()),
            min_importance: Some(5),
            ..Default::default()
        };
        let res_filtered = st.search_memories("oblivion", Some(&filter), 10).unwrap();
        assert_eq!(res_filtered.len(), 1);
        assert_eq!(res_filtered[0].importance, 5);

        // Empty query falls back to list
        let empty_search = st.search_memories("   ", None, 2).unwrap();
        assert_eq!(empty_search.len(), 2);
    }

    #[test]
    fn memory_fts5_trigger_synchronization() {
        let st = Store::in_memory().unwrap();

        let mem = st
            .add_memory(&NewMemory::new(
                "fact",
                "Initial text about quantum computing",
                3,
            ))
            .unwrap();

        // Search matches initial text
        let r1 = st.search_memories("quantum", None, 5).unwrap();
        assert_eq!(r1.len(), 1);

        // Update text
        st.update_memory(
            mem.id,
            Some("Updated text about relativistic mechanics"),
            None,
            None,
        )
        .unwrap();

        // Old keyword no longer matches
        let r_old = st.search_memories("quantum", None, 5).unwrap();
        assert_eq!(r_old.len(), 0);

        // New keyword matches
        let r_new = st.search_memories("relativistic", None, 5).unwrap();
        assert_eq!(r_new.len(), 1);

        // Delete removes from FTS
        st.delete_memory(mem.id).unwrap();
        let r_del = st.search_memories("relativistic", None, 5).unwrap();
        assert_eq!(r_del.len(), 0);
    }

    #[tokio::test]
    async fn memory_recall_project_scoping_and_isolation() {
        use oblivion_core::MemoryRecall;
        let st = Store::in_memory().unwrap();

        // 1. Global user preference (project = None)
        st.add_memory(&NewMemory::new(
            "user_pref",
            "User prefers snake_case for identifiers",
            5,
        ))
        .unwrap();

        // 2. Project Alpha fact
        st.add_memory(
            &NewMemory::new(
                "project_fact",
                "Alpha repository uses Tokio 1.40 runtime on port 3000",
                4,
            )
            .with_project("/workspace/alpha"),
        )
        .unwrap();

        // 3. Project Beta fact
        st.add_memory(
            &NewMemory::new(
                "project_fact",
                "Beta repository uses Async-std 1.12 runtime on port 5000",
                4,
            )
            .with_project("/workspace/beta"),
        )
        .unwrap();

        // Test Alpha recall: must include Alpha fact, must NEVER include Beta fact
        let alpha_recalled = st
            .recall_for_prompt("runtime and port settings", "/workspace/alpha", 2000)
            .await;
        let alpha_joined = alpha_recalled.join("\n");
        assert!(alpha_joined.contains("Alpha repository uses Tokio"));
        assert!(!alpha_joined.contains("Beta repository"));

        // Test Beta recall: must include Beta fact, must NEVER include Alpha fact
        let beta_recalled = st
            .recall_for_prompt("runtime and port settings", "/workspace/beta", 2000)
            .await;
        let beta_joined = beta_recalled.join("\n");
        assert!(beta_joined.contains("Beta repository uses Async-std"));
        assert!(!beta_joined.contains("Alpha repository"));

        // Test Gamma recall: must NEVER include Alpha or Beta facts
        let gamma_recalled = st
            .recall_for_prompt("runtime and port settings", "/workspace/gamma", 2000)
            .await;
        let gamma_joined = gamma_recalled.join("\n");
        assert!(!gamma_joined.contains("Alpha repository"));
        assert!(!gamma_joined.contains("Beta repository"));

        // Test global preference is accessible across all projects
        let global_in_alpha = st
            .recall_for_prompt("identifiers style", "/workspace/alpha", 2000)
            .await;
        assert!(global_in_alpha.join("\n").contains("snake_case"));
        let global_in_beta = st
            .recall_for_prompt("identifiers style", "/workspace/beta", 2000)
            .await;
        assert!(global_in_beta.join("\n").contains("snake_case"));
    }

    #[tokio::test]
    async fn memory_recall_respects_budget() {
        use oblivion_core::MemoryRecall;
        let st = Store::in_memory().unwrap();

        for i in 1..=10 {
            st.add_memory(&NewMemory::new(
                "fact",
                format!(
                    "Important architecture note number {i}: very long descriptive text details"
                ),
                3,
            ))
            .unwrap();
        }

        // Small budget (150 chars): should stop once budget is reached
        let small = st
            .recall_for_prompt("architecture note", "/workspace/test", 150)
            .await;
        assert!(!small.is_empty());
        let total_chars: usize = small.iter().map(|s| s.len() + 1).sum();
        assert!(total_chars <= 160);
        assert!(small.len() < 5);
    }

    #[test]
    fn memory_vector_storage_and_knn_search() {
        let st = Store::in_memory().unwrap();

        let m1 = st
            .add_memory(&NewMemory::new(
                "fact",
                "Frontend uses React and TypeScript",
                4,
            ))
            .unwrap();
        let m2 = st
            .add_memory(&NewMemory::new(
                "fact",
                "Backend uses Rust and Axum web framework",
                5,
            ))
            .unwrap();

        // 4-dimensional normalized vectors
        let v1 = [0.9f32, 0.1, 0.0, 0.0];
        let v2 = [0.0f32, 0.0, 0.9, 0.1];

        st.save_memory_embedding(m1.id, "test-model", &v1).unwrap();
        st.save_memory_embedding(m2.id, "test-model", &v2).unwrap();

        // Verify retrieval of embedding
        let fetched_v1 = st.get_memory_embedding(m1.id).unwrap().unwrap();
        assert_eq!(fetched_v1.len(), 4);
        assert!((fetched_v1[0] - 0.9).abs() < 1e-5);

        // Vector search close to v2 (backend/rust)
        let query_vec = [0.05f32, 0.0, 0.95, 0.05];
        let res = st.search_memories_vector(&query_vec, None, 5).unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].id, m2.id); // m2 is closest
        assert_eq!(res[1].id, m1.id);

        // Delete removes embedding
        st.delete_memory(m1.id).unwrap();
        assert!(st.get_memory_embedding(m1.id).unwrap().is_none());
        let res_after = st.search_memories_vector(&query_vec, None, 5).unwrap();
        assert_eq!(res_after.len(), 1);
        assert_eq!(res_after[0].id, m2.id);
    }

    #[tokio::test]
    async fn memory_vector_recall_with_embedder_and_fts_fallback() {
        use oblivion_core::MemoryRecall;
        let st = Store::in_memory().unwrap();
        let mock_embedder = Arc::new(MockEmbedder::new(8));

        let _m1 = st
            .add_memory(
                &NewMemory::new("project_fact", "Alpha repo uses React and Tailwind CSS", 4)
                    .with_project("/workspace/alpha"),
            )
            .unwrap();
        let m2 = st
            .add_memory(
                &NewMemory::new(
                    "project_fact",
                    "Alpha repo uses Rust with Tokio and Rusqlite",
                    5,
                )
                .with_project("/workspace/alpha"),
            )
            .unwrap();

        // Index vector for m2 only
        let v2 = mock_embedder.compute_vector(&m2.text);
        st.save_memory_embedding(m2.id, "mock-embed", &v2).unwrap();

        // Attach embedder
        st.set_embedder(mock_embedder.clone());

        // 1. Vector recall: query close to m2
        let recalled = st
            .recall_for_prompt("Rust with Tokio and Rusqlite", "/workspace/alpha", 2000)
            .await;
        let joined = recalled.join("\n");
        assert!(joined.contains("Rust with Tokio and Rusqlite"));

        // 2. FTS fallback: query matching m1 text (which has no vector indexed)
        // System must fall back to FTS5 and find m1!
        let recalled_fts = st
            .recall_for_prompt("Tailwind CSS", "/workspace/alpha", 2000)
            .await;
        let joined_fts = recalled_fts.join("\n");
        assert!(joined_fts.contains("React and Tailwind CSS"));
    }

    #[test]
    fn test_store_scheduled_tasks_crud_and_due_tasks() {
        let st = Store::in_memory().unwrap();

        // 1. Add tasks
        let past = chrono::Utc::now() - chrono::Duration::minutes(5);
        let future = chrono::Utc::now() + chrono::Duration::hours(2);

        let t1 = st
            .add_scheduled_task(
                &NewScheduledTask::new("Morning brief", "Summarize news", "0 9 * * *")
                    .with_workspace("/repos/jarvis"),
            )
            .unwrap();
        assert_eq!(t1.name, "Morning brief");
        assert!(t1.enabled);
        assert!(t1.is_recurring);

        // Update t1 to have next_run in the past (due now)
        let t1_updated = st
            .update_scheduled_task(
                &t1.id,
                &UpdateScheduledTask {
                    next_run: Some(Some(past)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(t1_updated.next_run, Some(past));

        let t2 = st
            .add_scheduled_task(
                &NewScheduledTask::new("One-off task", "Check deployment", "at-once")
                    .one_off(future),
            )
            .unwrap();
        assert!(!t2.is_recurring);
        assert_eq!(t2.next_run, Some(future));

        // 2. List tasks
        let all = st.list_scheduled_tasks(false).unwrap();
        assert_eq!(all.len(), 2);

        // 3. Due tasks
        let due = st.due_scheduled_tasks(chrono::Utc::now()).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, t1.id);

        // 4. Record task run
        st.record_task_run(&t1.id, "sess-123", "done", None, Some(future))
            .unwrap();
        let t1_after = st.get_scheduled_task(&t1.id).unwrap().unwrap();
        assert_eq!(t1_after.last_session.as_deref(), Some("sess-123"));
        assert_eq!(t1_after.last_outcome.as_deref(), Some("done"));
        assert_eq!(t1_after.next_run, Some(future));

        // 5. Delete task
        let deleted = st.delete_scheduled_task(&t2.id).unwrap();
        assert!(deleted);
        assert!(st.get_scheduled_task(&t2.id).unwrap().is_none());
    }
}
