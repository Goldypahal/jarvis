//! Memory types and query filters for Oblivion's long-term memory system.
//!
//! Memories store facts about the user, project facts, how-to recipes, and past outcomes.
//! Each memory tracks its type, source session, creation timestamp, last used timestamp,
//! and importance score (1 to 5). Full-text search is powered by SQLite FTS5.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Memory {
    pub id: i64,
    #[serde(rename = "type")]
    pub mem_type: String,
    pub text: String,
    pub source_session: Option<String>,
    pub project: Option<String>,
    pub created: String,
    pub last_used: String,
    pub importance: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMemory {
    #[serde(rename = "type")]
    pub mem_type: String,
    pub text: String,
    pub source_session: Option<String>,
    pub project: Option<String>,
    pub importance: u32,
}

impl NewMemory {
    pub fn new(mem_type: impl Into<String>, text: impl Into<String>, importance: u32) -> Self {
        Self {
            mem_type: mem_type.into(),
            text: text.into(),
            source_session: None,
            project: None,
            importance: importance.clamp(1, 5),
        }
    }

    pub fn with_source_session(mut self, session_id: impl Into<String>) -> Self {
        self.source_session = Some(session_id.into());
        self
    }

    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFilter {
    pub mem_type: Option<String>,
    pub source_session: Option<String>,
    pub project: Option<String>,
    pub min_importance: Option<u32>,
    pub include_global: bool,
}
