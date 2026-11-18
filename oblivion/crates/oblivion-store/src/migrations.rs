//! SQLite schema migrations for `oblivion.db`.
//!
//! Uses `PRAGMA user_version` to track schema versions incrementally:
//! - Version 1: `sessions` and `usage` tables (Month 1 baseline).
//! - Version 2: `memories` table, secondary indexes, FTS5 full-text index and sync triggers (Month 2).

use anyhow::Result;
use rusqlite::Connection;

pub const CURRENT_VERSION: i64 = 4;

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
}
