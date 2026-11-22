//! SQLite schema migrations for `oblivion.db`.
//!
//! Uses `PRAGMA user_version` to track schema versions incrementally:
//! - Version 1: `sessions` and `usage` tables (Month 1 baseline).
//! - Version 2: `memories` table, secondary indexes, FTS5 full-text index and sync triggers (Month 2).

use anyhow::Result;
use rusqlite::Connection;

pub const CURRENT_VERSION: i64 = 6;

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    let current_version: i64 = tx.query_row("PRAGMA user_version;", [], |r| r.get(0))?;

    if current_version < 1 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
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
            );",
        )?;
        tx.pragma_update(None, "user_version", 1)?;
    }

    if current_version < 2 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                mem_type       TEXT NOT NULL,
                text           TEXT NOT NULL,
                source_session TEXT,
                project        TEXT,
                created        TEXT NOT NULL,
                last_used      TEXT NOT NULL,
                importance     INTEGER NOT NULL DEFAULT 3
            );
            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(mem_type);
            CREATE INDEX IF NOT EXISTS idx_memories_last_used ON memories(last_used);
            CREATE INDEX IF NOT EXISTS idx_memories_importance ON memories(importance);
            CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project);

            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                text,
                content='memories',
                content_rowid='id'
            );

            CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.id, old.text);
            END;

            CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
                INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.id, old.text);
                INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
            END;",
        )?;
        tx.pragma_update(None, "user_version", 2)?;
    }

    if current_version < 3 {
        let has_project: bool = tx
            .query_row(
                "SELECT count(*) FROM pragma_table_info('memories') WHERE name = 'project'",
                [],
                |r| r.get(0),
            )
            .map(|c: i64| c > 0)
            .unwrap_or(false);

        if !has_project {
            tx.execute_batch(
                "ALTER TABLE memories ADD COLUMN project TEXT;
                 CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project);",
            )?;
        }
        tx.pragma_update(None, "user_version", 3)?;
    }

    if current_version < 4 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_embeddings (
                memory_id   INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
                model       TEXT NOT NULL,
                dimensions  INTEGER NOT NULL,
                embedding   BLOB NOT NULL,
                created     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memory_embeddings_model ON memory_embeddings(model);",
        )?;
        tx.pragma_update(None, "user_version", 4)?;
    }

    if current_version < 5 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS scheduled_tasks (
                id           TEXT PRIMARY KEY,
                name         TEXT NOT NULL,
                prompt       TEXT NOT NULL,
                schedule     TEXT NOT NULL,
                is_recurring INTEGER NOT NULL DEFAULT 1,
                next_run     TEXT,
                workspace    TEXT,
                enabled      INTEGER NOT NULL DEFAULT 1,
                last_run     TEXT,
                last_outcome TEXT,
                last_error   TEXT,
                last_session TEXT,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_enabled ON scheduled_tasks(enabled);
            CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_next_run ON scheduled_tasks(next_run);",
        )?;
        tx.pragma_update(None, "user_version", 5)?;
    }

    if current_version < 6 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_approvals (
                id          TEXT PRIMARY KEY,
                task_id     TEXT,
                session_id  TEXT NOT NULL,
                tool_name   TEXT NOT NULL,
                summary     TEXT NOT NULL,
                risk        TEXT NOT NULL DEFAULT 'ask',
                status      TEXT NOT NULL DEFAULT 'pending',
                created_at  TEXT NOT NULL,
                resolved_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_pending_approvals_status ON pending_approvals(status);
            CREATE INDEX IF NOT EXISTS idx_pending_approvals_session ON pending_approvals(session_id);
            CREATE INDEX IF NOT EXISTS idx_pending_approvals_created_at ON pending_approvals(created_at);

            CREATE TABLE IF NOT EXISTS notifications (
                id          TEXT PRIMARY KEY,
                title       TEXT NOT NULL,
                body        TEXT NOT NULL,
                kind        TEXT NOT NULL,
                task_id     TEXT,
                session_id  TEXT,
                approval_id TEXT,
                read        INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_notifications_read ON notifications(read);
            CREATE INDEX IF NOT EXISTS idx_notifications_created_at ON notifications(created_at);",
        )?;
        tx.pragma_update(None, "user_version", 6)?;
    }

    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_from_scratch_creates_all_tables_and_sets_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"usage".to_string()));
        assert!(tables.contains(&"memories".to_string()));
        assert!(tables.contains(&"memories_fts".to_string()));
        assert!(tables.contains(&"memory_embeddings".to_string()));
        assert!(tables.contains(&"scheduled_tasks".to_string()));
        assert!(tables.contains(&"pending_approvals".to_string()));
        assert!(tables.contains(&"notifications".to_string()));
    }

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        // Second call must succeed with no changes
        migrate(&mut conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);
    }

    #[test]
    fn migration_upgrades_v1_database_preserving_existing_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        // Simulate v1 database
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, created TEXT, updated TEXT, workspace TEXT, title TEXT, messages TEXT);
             CREATE TABLE usage (day TEXT, target TEXT, requests INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER, PRIMARY KEY (day, target));
             PRAGMA user_version = 1;
             INSERT INTO sessions (id, created, updated, workspace, title, messages) VALUES ('s1', 'now', 'now', '/tmp', 'Test', '[]');
             INSERT INTO usage (day, target, requests) VALUES ('2026-11-18', 'groq/test', 5);"
        ).unwrap();

        // Run migration to v3
        migrate(&mut conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);

        // Verify existing data preserved
        let s_count: i64 = conn
            .query_row("SELECT count(*) FROM sessions WHERE id = 's1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(s_count, 1);
        let u_count: i64 = conn
            .query_row(
                "SELECT requests FROM usage WHERE target = 'groq/test'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(u_count, 5);

        // Verify memories table works with project column
        conn.execute(
            "INSERT INTO memories (mem_type, text, project, created, last_used, importance) VALUES ('fact', 'user likes rust', 'proj-1', 'now', 'now', 5)",
            [],
        ).unwrap();
        let fts_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH 'rust'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);
    }

    #[test]
    fn migration_upgrades_v4_to_v5_preserving_existing_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        // Setup schema up to version 4
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, created TEXT, updated TEXT, workspace TEXT, title TEXT, messages TEXT);
             CREATE TABLE usage (day TEXT, target TEXT, requests INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER, PRIMARY KEY (day, target));
             CREATE TABLE memories (id INTEGER PRIMARY KEY AUTOINCREMENT, mem_type TEXT, text TEXT, source_session TEXT, project TEXT, created TEXT, last_used TEXT, importance INTEGER DEFAULT 3);
             CREATE VIRTUAL TABLE memories_fts USING fts5(text, content='memories', content_rowid='id');
             CREATE TABLE memory_embeddings (memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE, model TEXT, dimensions INTEGER, embedding BLOB, created TEXT);
             PRAGMA user_version = 4;
             INSERT INTO sessions (id, created, updated, workspace, title, messages) VALUES ('s1', 'now', 'now', '/tmp', 'Test', '[]');"
        ).unwrap();

        migrate(&mut conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, CURRENT_VERSION);

        // Verify existing session preserved
        let s_count: i64 = conn
            .query_row("SELECT count(*) FROM sessions WHERE id = 's1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(s_count, 1);

        // Verify scheduled_tasks exists and accepts inserts
        conn.execute(
            "INSERT INTO scheduled_tasks (id, name, prompt, schedule, created_at, updated_at) VALUES ('t1', 'Brief', 'Check email', '0 9 * * *', 'now', 'now')",
            [],
        ).unwrap();
        let t_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM scheduled_tasks WHERE id = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t_count, 1);
    }

    #[test]
    fn migration_upgrades_v5_to_v6_preserving_existing_data() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, created TEXT, updated TEXT, workspace TEXT, title TEXT, messages TEXT);
             CREATE TABLE usage (day TEXT, target TEXT, requests INTEGER, prompt_tokens INTEGER, completion_tokens INTEGER, PRIMARY KEY (day, target));
             CREATE TABLE memories (id INTEGER PRIMARY KEY AUTOINCREMENT, mem_type TEXT, text TEXT, source_session TEXT, project TEXT, created TEXT, last_used TEXT, importance INTEGER DEFAULT 3);
             CREATE VIRTUAL TABLE memories_fts USING fts5(text, content='memories', content_rowid='id');
             CREATE TABLE memory_embeddings (memory_id INTEGER PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE, model TEXT, dimensions INTEGER, embedding BLOB, created TEXT);
             CREATE TABLE scheduled_tasks (id TEXT PRIMARY KEY, name TEXT NOT NULL, prompt TEXT NOT NULL, schedule TEXT NOT NULL, is_recurring INTEGER NOT NULL DEFAULT 1, next_run TEXT, workspace TEXT, enabled INTEGER NOT NULL DEFAULT 1, last_run TEXT, last_outcome TEXT, last_error TEXT, last_session TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             PRAGMA user_version = 5;
             INSERT INTO scheduled_tasks (id, name, prompt, schedule, created_at, updated_at) VALUES ('t1', 'Brief', 'Check email', '0 9 * * *', 'now', 'now');"
        ).unwrap();

        migrate(&mut conn).unwrap();

        let v: i64 = conn
            .query_row("PRAGMA user_version;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 6);

        // Verify existing scheduled task preserved
        let t_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM scheduled_tasks WHERE id = 't1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(t_count, 1);

        // Verify pending_approvals accepts inserts
        conn.execute(
            "INSERT INTO pending_approvals (id, task_id, session_id, tool_name, summary, risk, status, created_at)
             VALUES ('appr-1', 't1', 'sess-1', 'shell', 'cargo build', 'ask', 'pending', 'now')",
            [],
        ).unwrap();

        let a_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM pending_approvals WHERE id = 'appr-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(a_count, 1);

        // Verify notifications accepts inserts
        conn.execute(
            "INSERT INTO notifications (id, title, body, kind, task_id, session_id, approval_id, read, created_at)
             VALUES ('notif-1', 'Approval needed', 'Run cargo build', 'approval_needed', 't1', 'sess-1', 'appr-1', 0, 'now')",
            [],
        ).unwrap();

        let n_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM notifications WHERE id = 'notif-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_count, 1);
    }
}
