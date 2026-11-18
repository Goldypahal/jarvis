//! SQLite storage (SRS FR-53, router quota ledger): one file,
//! `~/.oblivion/oblivion.db`, holding saved sessions, per-day usage of
//! every model so free-tier daily limits survive restarts, and long-term memory.

pub mod memory;
pub mod migrations;
pub mod tools;

pub use memory::{Memory, MemoryFilter, NewMemory};
pub use tools::MemoryTools;

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use oblivion_core::session::{Session, SessionInfo, SessionStore};
use oblivion_providers::{Role, Usage, UsageLedger};
use rusqlite::{params, Connection, OptionalExtension};

pub struct Store {
    conn: Mutex<Connection>,
}

/// Today's usage of one model.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    for word in query.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        if !word.is_empty() {
            terms.push(format!("\"{word}\"*"));
        }
    }
    terms.join(" ")
}

fn row_to_memory(r: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        mem_type: r.get(1)?,
        text: r.get(2)?,
        source_session: r.get(3)?,
        created: r.get(4)?,
        last_used: r.get(5)?,
        importance: r.get::<_, i64>(6)? as u32,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
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
        })
    }

    pub fn in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        migrations::migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
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
            "SELECT id, created, workspace, title FROM sessions ORDER BY updated DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(SessionInfo {
                    id: r.get(0)?,
                    created: r.get(1)?,
                    workspace: r.get::<_, String>(2)?.into(),
                    first_prompt: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
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
            "INSERT INTO memories (mem_type, text, source_session, created, last_used, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                mem.mem_type,
                mem.text,
                mem.source_session,
                now,
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
                "SELECT id, mem_type, text, source_session, created, last_used, importance
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
                "SELECT id, mem_type, text, source_session, created, last_used, importance
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
        let mut sql = "SELECT id, mem_type, text, source_session, created, last_used, importance FROM memories WHERE 1=1".to_string();
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
        let mut sql = "SELECT m.id, m.mem_type, m.text, m.source_session, m.created, m.last_used, m.importance
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

#[cfg(test)]
mod tests {
    use super::*;
    use oblivion_providers::Message;

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
        assert!(st.load_session("nope").is_err());
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
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM test_fts WHERE test_fts MATCH 'hello';",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
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
}
