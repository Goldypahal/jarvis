//! Conversation history saved as JSON so a session can be resumed (SRS FR-53;
//! SQLite storage replaces this in Phase 1).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use oblivion_providers::{Message, Role};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created: String,
    pub workspace: PathBuf,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub created: String,
    pub workspace: PathBuf,
    pub first_prompt: String,
}

/// Where sessions are saved. The CLI uses SQLite; tests and fallbacks use JSON files.
pub trait SessionStore: Send + Sync {
    fn save(&self, session: &Session) -> Result<()>;
}

/// One JSON file per session in a folder (the Phase 0 format).
pub struct JsonDir(pub PathBuf);

impl SessionStore for JsonDir {
    fn save(&self, session: &Session) -> Result<()> {
        session.save(&self.0)
    }
}

impl Session {
    pub fn new(workspace: PathBuf) -> Self {
        let now = chrono::Local::now();
        Self {
            id: now.format("%Y%m%d-%H%M%S").to_string(),
            created: now.to_rfc3339(),
            workspace,
            messages: Vec::new(),
        }
    }

    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(format!("{}.json", self.id))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{}.json.tmp", self.id));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, self.path_in(dir))?;
        Ok(())
    }

    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let path = dir.join(format!("{id}.json"));
        let text = std::fs::read_to_string(&path).with_context(|| format!("no session {id}"))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Most recent first.
    pub fn list(dir: &Path) -> Vec<SessionInfo> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return out;
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
            let first_prompt = s
                .messages
                .iter()
                .find(|m| m.role == Role::User)
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
                .chars()
                .take(70)
                .collect();
            out.push(SessionInfo {
                id: s.id,
                created: s.created,
                workspace: s.workspace,
                first_prompt,
            });
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_list() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Session::new(PathBuf::from("/w"));
        s.messages.push(Message::system("sys"));
        s.messages.push(Message::user("fix the bug"));
        s.save(d.path()).unwrap();
        let back = Session::load(d.path(), &s.id).unwrap();
        assert_eq!(back.messages, s.messages);
        let list = Session::list(d.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].first_prompt, "fix the bug");
    }
}
