//! Scheduled tasks storage and queries in `oblivion.db`.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    pub prompt: String,
    /// e.g. "0 9 * * *" or "every 2h" or "2026-11-22T09:00:00Z"
    pub schedule: String,
    pub is_recurring: bool,
    pub next_run: Option<DateTime<Utc>>,
    pub workspace: Option<String>,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub last_outcome: Option<String>,
    pub last_error: Option<String>,
    pub last_session: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewScheduledTask {
    pub id: Option<String>,
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    pub is_recurring: bool,
    pub next_run: Option<DateTime<Utc>>,
    pub workspace: Option<String>,
    pub enabled: Option<bool>,
}

impl NewScheduledTask {
    pub fn new(
        name: impl Into<String>,
        prompt: impl Into<String>,
        schedule: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            name: name.into(),
            prompt: prompt.into(),
            schedule: schedule.into(),
            is_recurring: true,
            next_run: None,
            workspace: None,
            enabled: Some(true),
        }
    }

    pub fn with_workspace(mut self, workspace: impl Into<String>) -> Self {
        self.workspace = Some(workspace.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn one_off(mut self, run_at: DateTime<Utc>) -> Self {
        self.is_recurring = false;
        self.next_run = Some(run_at);
        self.schedule = run_at.to_rfc3339();
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateScheduledTask {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<String>,
    pub is_recurring: Option<bool>,
    pub next_run: Option<Option<DateTime<Utc>>>,
    pub workspace: Option<Option<String>>,
    pub enabled: Option<bool>,
}

fn parse_dt(s: Option<String>) -> Option<DateTime<Utc>> {
    s.and_then(|v| {
        DateTime::parse_from_rfc3339(&v)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledTask> {
    let next_run_str: Option<String> = row.get(5)?;
    let last_run_str: Option<String> = row.get(8)?;
    let created_str: String = row.get(12)?;
    let updated_str: String = row.get(13)?;

    let created_at = DateTime::parse_from_rfc3339(&created_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_str)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let is_recurring_int: i64 = row.get(4)?;
    let enabled_int: i64 = row.get(7)?;

    Ok(ScheduledTask {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        schedule: row.get(3)?,
        is_recurring: is_recurring_int != 0,
        next_run: parse_dt(next_run_str),
        workspace: row.get(6)?,
        enabled: enabled_int != 0,
        last_run: parse_dt(last_run_str),
        last_outcome: row.get(9)?,
        last_error: row.get(10)?,
        last_session: row.get(11)?,
        created_at,
        updated_at,
    })
}

pub(crate) fn db_add_task(conn: &Connection, task: &NewScheduledTask) -> Result<ScheduledTask> {
    let id = task.id.clone().unwrap_or_else(|| {
        let nanos = Utc::now().timestamp_nanos_opt().unwrap_or(0);
        format!("task-{:x}", nanos)
    });
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let next_run_str = task.next_run.map(|d| d.to_rfc3339());
    let enabled = task.enabled.unwrap_or(true);

    conn.execute(
        "INSERT INTO scheduled_tasks (
            id, name, prompt, schedule, is_recurring, next_run, workspace, enabled,
            last_run, last_outcome, last_error, last_session, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, NULL, ?9, ?9)",
        params![
            id,
            task.name,
            task.prompt,
            task.schedule,
            if task.is_recurring { 1 } else { 0 },
            next_run_str,
            task.workspace,
            if enabled { 1 } else { 0 },
            now_str,
        ],
    )
    .with_context(|| format!("failed to insert scheduled task '{id}'"))?;

    Ok(ScheduledTask {
        id,
        name: task.name.clone(),
        prompt: task.prompt.clone(),
        schedule: task.schedule.clone(),
        is_recurring: task.is_recurring,
        next_run: task.next_run,
        workspace: task.workspace.clone(),
        enabled,
        last_run: None,
        last_outcome: None,
        last_error: None,
        last_session: None,
        created_at: now,
        updated_at: now,
    })
}

pub(crate) fn db_get_task(conn: &Connection, id: &str) -> Result<Option<ScheduledTask>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, schedule, is_recurring, next_run, workspace, enabled,
                last_run, last_outcome, last_error, last_session, created_at, updated_at
         FROM scheduled_tasks WHERE id = ?1",
    )?;
    stmt.query_row(params![id], row_to_task)
        .optional()
        .context("querying scheduled task")
}

pub(crate) fn db_list_tasks(conn: &Connection, enabled_only: bool) -> Result<Vec<ScheduledTask>> {
    let sql = if enabled_only {
        "SELECT id, name, prompt, schedule, is_recurring, next_run, workspace, enabled,
                last_run, last_outcome, last_error, last_session, created_at, updated_at
         FROM scheduled_tasks WHERE enabled = 1 ORDER BY next_run ASC, created_at DESC"
    } else {
        "SELECT id, name, prompt, schedule, is_recurring, next_run, workspace, enabled,
                last_run, last_outcome, last_error, last_session, created_at, updated_at
         FROM scheduled_tasks ORDER BY created_at DESC"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], row_to_task)?;
    let mut tasks = Vec::new();
    for r in rows {
        tasks.push(r?);
    }
    Ok(tasks)
}

pub(crate) fn db_due_tasks(conn: &Connection, now: DateTime<Utc>) -> Result<Vec<ScheduledTask>> {
    let now_str = now.to_rfc3339();
    let mut stmt = conn.prepare(
        "SELECT id, name, prompt, schedule, is_recurring, next_run, workspace, enabled,
                last_run, last_outcome, last_error, last_session, created_at, updated_at
         FROM scheduled_tasks
         WHERE enabled = 1 AND next_run IS NOT NULL AND next_run <= ?1
         ORDER BY next_run ASC",
    )?;
    let rows = stmt.query_map(params![now_str], row_to_task)?;
    let mut tasks = Vec::new();
    for r in rows {
        tasks.push(r?);
    }
    Ok(tasks)
}

pub(crate) fn db_update_task(
    conn: &Connection,
    id: &str,
    updates: &UpdateScheduledTask,
) -> Result<ScheduledTask> {
    let existing =
        db_get_task(conn, id)?.ok_or_else(|| anyhow::anyhow!("scheduled task '{id}' not found"))?;

    let name = updates.name.as_deref().unwrap_or(&existing.name);
    let prompt = updates.prompt.as_deref().unwrap_or(&existing.prompt);
    let schedule = updates.schedule.as_deref().unwrap_or(&existing.schedule);
    let is_recurring = updates.is_recurring.unwrap_or(existing.is_recurring);
    let next_run = match &updates.next_run {
        Some(opt) => *opt,
        None => existing.next_run,
    };
    let workspace = match &updates.workspace {
        Some(opt) => opt.clone(),
        None => existing.workspace.clone(),
    };
    let enabled = updates.enabled.unwrap_or(existing.enabled);
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let next_run_str = next_run.map(|d| d.to_rfc3339());

    conn.execute(
        "UPDATE scheduled_tasks SET
            name = ?1,
            prompt = ?2,
            schedule = ?3,
            is_recurring = ?4,
            next_run = ?5,
            workspace = ?6,
            enabled = ?7,
            updated_at = ?8
         WHERE id = ?9",
        params![
            name,
            prompt,
            schedule,
            if is_recurring { 1 } else { 0 },
            next_run_str,
            workspace,
            if enabled { 1 } else { 0 },
            now_str,
            id,
        ],
    )
    .with_context(|| format!("failed to update scheduled task '{id}'"))?;

    Ok(ScheduledTask {
        id: id.to_string(),
        name: name.to_string(),
        prompt: prompt.to_string(),
        schedule: schedule.to_string(),
        is_recurring,
        next_run,
        workspace,
        enabled,
        last_run: existing.last_run,
        last_outcome: existing.last_outcome,
        last_error: existing.last_error,
        last_session: existing.last_session,
        created_at: existing.created_at,
        updated_at: now,
    })
}

pub(crate) fn db_delete_task(conn: &Connection, id: &str) -> Result<bool> {
    let count = conn.execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
    Ok(count > 0)
}

pub(crate) fn db_record_task_run(
    conn: &Connection,
    id: &str,
    session_id: &str,
    outcome: &str,
    error: Option<&str>,
    next_run: Option<DateTime<Utc>>,
) -> Result<()> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let next_run_str = next_run.map(|d| d.to_rfc3339());

    // If next_run is None, we also set enabled = 0 if it was one-off
    conn.execute(
        "UPDATE scheduled_tasks SET
            last_run = ?1,
            last_session = ?2,
            last_outcome = ?3,
            last_error = ?4,
            next_run = ?5,
            updated_at = ?1,
            enabled = CASE WHEN is_recurring = 0 AND ?5 IS NULL THEN 0 ELSE enabled END
         WHERE id = ?6",
        params![now_str, session_id, outcome, error, next_run_str, id],
    )
    .with_context(|| format!("recording run for task '{id}'"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_serialization_roundtrip() {
        let task = ScheduledTask {
            id: "task-123".into(),
            name: "Morning brief".into(),
            prompt: "Summarize issues".into(),
            schedule: "0 9 * * *".into(),
            is_recurring: true,
            next_run: Some(Utc::now()),
            workspace: Some("/tmp/project".into()),
            enabled: true,
            last_run: None,
            last_outcome: None,
            last_error: None,
            last_session: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let json = serde_json::to_string(&task).unwrap();
        let parsed: ScheduledTask = serde_json::from_str(&json).unwrap();
        assert_eq!(task.id, parsed.id);
        assert_eq!(task.name, parsed.name);
        assert_eq!(task.is_recurring, parsed.is_recurring);
    }
}
