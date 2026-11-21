//! Pending approvals queue and notification store for unattended runs.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use oblivion_core::agent::{Approver, Decision};
use oblivion_core::is_low_risk_action;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::Store;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub task_id: Option<String>,
    pub session_id: String,
    pub tool_name: String,
    pub summary: String,
    pub risk: String,
    pub status: String, // "pending", "approved", "denied"
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPendingApproval {
    pub id: Option<String>,
    pub task_id: Option<String>,
    pub session_id: String,
    pub tool_name: String,
    pub summary: String,
    pub risk: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: String, // "approval_needed", "run_finished", "run_failed", "info"
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub approval_id: Option<String>,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewNotification {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub task_id: Option<String>,
    pub session_id: Option<String>,
    pub approval_id: Option<String>,
}

fn row_to_approval(row: &Row<'_>) -> rusqlite::Result<PendingApproval> {
    let id: String = row.get(0)?;
    let task_id: Option<String> = row.get(1)?;
    let session_id: String = row.get(2)?;
    let tool_name: String = row.get(3)?;
    let summary: String = row.get(4)?;
    let risk: String = row.get(5)?;
    let status: String = row.get(6)?;
    let created_at_str: String = row.get(7)?;
    let resolved_at_str: Option<String> = row.get(8)?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let resolved_at = resolved_at_str.and_then(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    Ok(PendingApproval {
        id,
        task_id,
        session_id,
        tool_name,
        summary,
        risk,
        status,
        created_at,
        resolved_at,
    })
}

fn row_to_notification(row: &Row<'_>) -> rusqlite::Result<Notification> {
    let id: String = row.get(0)?;
    let title: String = row.get(1)?;
    let body: String = row.get(2)?;
    let kind: String = row.get(3)?;
    let task_id: Option<String> = row.get(4)?;
    let session_id: Option<String> = row.get(5)?;
    let approval_id: Option<String> = row.get(6)?;
    let read_val: i64 = row.get(7)?;
    let created_at_str: String = row.get(8)?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(Notification {
        id,
        title,
        body,
        kind,
        task_id,
        session_id,
        approval_id,
        read: read_val != 0,
        created_at,
    })
}

// ---------------- Pending Approvals DB helpers ----------------

pub fn db_add_approval(conn: &Connection, new: &NewPendingApproval) -> Result<PendingApproval> {
    let now = Utc::now();
    let id = new
        .id
        .clone()
        .unwrap_or_else(|| format!("appr-{:x}", now.timestamp_nanos_opt().unwrap_or(0)));
    let risk = new.risk.clone().unwrap_or_else(|| "ask".to_string());
    let status = "pending".to_string();
    let created_at_str = now.to_rfc3339();

    conn.execute(
        "INSERT INTO pending_approvals (id, task_id, session_id, tool_name, summary, risk, status, created_at, resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
        params![
            id,
            new.task_id,
            new.session_id,
            new.tool_name,
            new.summary,
            risk,
            status,
            created_at_str,
        ],
    ).context("inserting pending approval")?;

    Ok(PendingApproval {
        id,
        task_id: new.task_id.clone(),
        session_id: new.session_id.clone(),
        tool_name: new.tool_name.clone(),
        summary: new.summary.clone(),
        risk,
        status,
        created_at: now,
        resolved_at: None,
    })
}

pub fn db_get_approval(conn: &Connection, id: &str) -> Result<Option<PendingApproval>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, tool_name, summary, risk, status, created_at, resolved_at
         FROM pending_approvals WHERE id = ?1",
    )?;

    let approval = stmt.query_row(params![id], row_to_approval).optional()?;
    Ok(approval)
}

pub fn db_list_approvals(conn: &Connection, status: Option<&str>) -> Result<Vec<PendingApproval>> {
    let sql = match status {
        Some(_) => "SELECT id, task_id, session_id, tool_name, summary, risk, status, created_at, resolved_at
                    FROM pending_approvals WHERE status = ?1 ORDER BY created_at DESC",
        None => "SELECT id, task_id, session_id, tool_name, summary, risk, status, created_at, resolved_at
                 FROM pending_approvals ORDER BY created_at DESC",
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = if let Some(s) = status {
        stmt.query_map(params![s], row_to_approval)?
    } else {
        stmt.query_map([], row_to_approval)?
    };

    let mut approvals = Vec::new();
    for row in rows {
        approvals.push(row?);
    }
    Ok(approvals)
}

pub fn db_resolve_approval(conn: &Connection, id: &str, status: &str) -> Result<PendingApproval> {
    let normalized_status = match status.to_lowercase().as_str() {
        "approve" | "approved" | "yes" => "approved",
        "deny" | "denied" | "no" | "reject" | "rejected" => "denied",
        other => bail!("Invalid approval resolution status: {other}"),
    };

    let now = Utc::now();
    let resolved_at_str = now.to_rfc3339();

    let rows_affected = conn.execute(
        "UPDATE pending_approvals SET status = ?1, resolved_at = ?2 WHERE id = ?3 AND status = 'pending'",
        params![normalized_status, resolved_at_str, id],
    )?;

    if rows_affected == 0 {
        let existing = db_get_approval(conn, id)?;
        match existing {
            Some(appr) => return Ok(appr),
            None => bail!("Pending approval not found: {id}"),
        }
    }

    db_get_approval(conn, id)?.context("retrieving updated approval")
}

// ---------------- Notifications DB helpers ----------------

pub fn db_add_notification(conn: &Connection, new: &NewNotification) -> Result<Notification> {
    let now = Utc::now();
    let id = new
        .id
        .clone()
        .unwrap_or_else(|| format!("notif-{:x}", now.timestamp_nanos_opt().unwrap_or(0)));
    let created_at_str = now.to_rfc3339();

    conn.execute(
        "INSERT INTO notifications (id, title, body, kind, task_id, session_id, approval_id, read, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)",
        params![
            id,
            new.title,
            new.body,
            new.kind,
            new.task_id,
            new.session_id,
            new.approval_id,
            created_at_str,
        ],
    ).context("inserting notification")?;

    Ok(Notification {
        id,
        title: new.title.clone(),
        body: new.body.clone(),
        kind: new.kind.clone(),
        task_id: new.task_id.clone(),
        session_id: new.session_id.clone(),
        approval_id: new.approval_id.clone(),
        read: false,
        created_at: now,
    })
}

pub fn db_get_notification(conn: &Connection, id: &str) -> Result<Option<Notification>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, body, kind, task_id, session_id, approval_id, read, created_at
         FROM notifications WHERE id = ?1",
    )?;

    let notif = stmt
        .query_row(params![id], row_to_notification)
        .optional()?;
    Ok(notif)
}

pub fn db_list_notifications(
    conn: &Connection,
    unread_only: bool,
    limit: Option<usize>,
) -> Result<Vec<Notification>> {
    let limit_clause = match limit {
        Some(l) => format!("LIMIT {}", l),
        None => "".to_string(),
    };

    let sql = if unread_only {
        format!(
            "SELECT id, title, body, kind, task_id, session_id, approval_id, read, created_at
             FROM notifications WHERE read = 0 ORDER BY created_at DESC {}",
            limit_clause
        )
    } else {
        format!(
            "SELECT id, title, body, kind, task_id, session_id, approval_id, read, created_at
             FROM notifications ORDER BY created_at DESC {}",
            limit_clause
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_notification)?;
    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

pub fn db_mark_notification_read(conn: &Connection, id: &str) -> Result<bool> {
    let count = conn.execute(
        "UPDATE notifications SET read = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(count > 0)
}

pub fn db_mark_all_notifications_read(conn: &Connection) -> Result<usize> {
    let count = conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])?;
    Ok(count)
}

// ---------------- UnattendedQueueApprover ----------------

/// An approver for unattended scheduled task runs that:
/// 1. Automatically permits low-risk read-only actions (recalls, searches, fetch).
/// 2. Records a persistent `PendingApproval` and an `approval_needed` notification in SQLite store
///    for any risky action, and declines execution (`Decision::No`) so the background runner
///    remains safe and does not block indefinitely.
pub struct UnattendedQueueApprover {
    store: Arc<Store>,
    session_id: String,
    task_id: Option<String>,
}

impl UnattendedQueueApprover {
    pub fn new(store: Arc<Store>, session_id: impl Into<String>, task_id: Option<String>) -> Self {
        Self {
            store,
            session_id: session_id.into(),
            task_id,
        }
    }
}

impl Approver for UnattendedQueueApprover {
    fn approve(&self, tool: &str, summary: &str) -> Decision {
        if is_low_risk_action(tool, summary) {
            Decision::Yes
        } else {
            let now = Utc::now();
            let approval_id = format!("appr-{:x}", now.timestamp_nanos_opt().unwrap_or(0));

            let _ = self.store.add_pending_approval(&NewPendingApproval {
                id: Some(approval_id.clone()),
                task_id: self.task_id.clone(),
                session_id: self.session_id.clone(),
                tool_name: tool.to_string(),
                summary: summary.to_string(),
                risk: Some("ask".to_string()),
            });

            let notif_id = format!("notif-{:x}", now.timestamp_nanos_opt().unwrap_or(0));

            let _ = self.store.add_notification(&NewNotification {
                id: Some(notif_id),
                title: format!("Approval needed: {}", tool),
                body: format!("Action '{}' requires user approval.", summary),
                kind: "approval_needed".to_string(),
                task_id: self.task_id.clone(),
                session_id: Some(self.session_id.clone()),
                approval_id: Some(approval_id),
            });

            Decision::No
        }
    }
}
