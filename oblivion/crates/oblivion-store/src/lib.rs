//! SQLite storage (SRS FR-53, router quota ledger): one file,
//! `~/.oblivion/oblivion.db`, holding saved sessions and per-day usage of
//! every model so free-tier daily limits survive restarts.

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

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id        TEXT PRIMARY KEY,
    created   TEXT NOT NULL,
    updated   TEXT NOT NULL,
    workspace TEXT NOT NULL,
    title     TEXT NOT NULL,
    messages  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS usage (
    day               TEXT NOT NULL,
    target            TEXT NOT NULL,
    requests          INTEGER NOT NULL DEFAULT 0,
    prompt_tokens     INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, target)
);
";

/// Free tiers reset on UTC days.
fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 3000)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
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
}
