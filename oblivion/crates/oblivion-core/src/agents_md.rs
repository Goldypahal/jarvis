//! Support for reading AGENTS.md at start and proposing updates at session end.
//!
//! AGENTS.md provides persistent, repository-level guidelines and facts
//! that the agent reads into its system prompt on every startup.
//! When new project facts or reusable recipes are learned during a session,
//! Oblivion compares them against AGENTS.md and offers a structured update.

use crate::extract::ProposedMemory;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A proposed update to an AGENTS.md file in a workspace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentsMdProposal {
    /// Target file path (typically `<workspace>/AGENTS.md`).
    pub path: PathBuf,
    /// Whether the file already existed before this update.
    pub exists: bool,
    /// The individual new facts or instructions being added.
    pub additions: Vec<String>,
    /// The complete updated file content ready to be written.
    pub updated_content: String,
}

/// Propose an update to `AGENTS.md` in `root` based on extracted memories.
///
/// Returns `Some(proposal)` if there are new project facts or recipes not already
/// documented in `AGENTS.md`. Returns `None` if no qualifying facts were found
/// or if all facts are already present in the file.
pub fn propose_update(root: &Path, memories: &[ProposedMemory]) -> Option<AgentsMdProposal> {
    let target = root.join("AGENTS.md");
    let exists = target.is_file();
    let existing_content = if exists {
        std::fs::read_to_string(&target).unwrap_or_default()
    } else {
        String::new()
    };
    let existing_lower = existing_content.to_lowercase();

    let mut additions = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for m in memories {
        // Only project facts, recipes, or critical environment facts belong in AGENTS.md
        let is_candidate = m.mem_type == "project_fact"
            || m.mem_type == "recipe"
            || (m.mem_type == "fact" && m.importance >= 4);

        if !is_candidate {
            continue;
        }

        let trimmed = m.text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();
        // Check if this fact is already mentioned in AGENTS.md
        if existing_lower.contains(&lower) {
            continue;
        }

        if seen.insert(lower) {
            additions.push(trimmed.to_string());
        }
    }

    if additions.is_empty() {
        return None;
    }

    let updated_content = format_updated_content(&existing_content, &additions);

    Some(AgentsMdProposal {
        path: target,
        exists,
        additions,
        updated_content,
    })
}

/// Formats the updated AGENTS.md content cleanly preserving existing sections.
fn format_updated_content(existing: &str, additions: &[String]) -> String {
    let trimmed_existing = existing.trim_end();
    if trimmed_existing.is_empty() {
        let mut out = String::from("# Project Instructions\n\n## Learned Project Facts\n");
        for add in additions {
            out.push_str(&format!("- {add}\n"));
        }
        return out;
    }

    let section_header = "## Learned Project Facts";
    if let Some(pos) = trimmed_existing.find(section_header) {
        // Section already exists; insert additions right after header or at end of section
        let after_header = pos + section_header.len();
        let mut out = String::with_capacity(existing.len() + 256);
        out.push_str(&trimmed_existing[..after_header]);
        out.push('\n');
        for add in additions {
            out.push_str(&format!("- {add}\n"));
        }
        out.push_str(trimmed_existing[after_header..].trim_start_matches('\n'));
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    } else {
        let mut out = String::with_capacity(trimmed_existing.len() + 256);
        out.push_str(trimmed_existing);
        out.push_str("\n\n## Learned Project Facts\n");
        for add in additions {
            out.push_str(&format!("- {add}\n"));
        }
        out
    }
}

/// Apply a proposal by writing the updated content to disk.
pub fn apply_update(proposal: &AgentsMdProposal) -> std::io::Result<()> {
    if let Some(parent) = proposal.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&proposal.path, &proposal.updated_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_updated_content_empty() {
        let res = format_updated_content("", &["Fact A".into(), "Fact B".into()]);
        assert_eq!(
            res,
            "# Project Instructions\n\n## Learned Project Facts\n- Fact A\n- Fact B\n"
        );
    }

    #[test]
    fn test_format_updated_content_append_new_section() {
        let existing = "# My Project\n\n- Build with cargo build\n";
        let res = format_updated_content(existing, &["New fact".into()]);
        assert_eq!(
            res,
            "# My Project\n\n- Build with cargo build\n\n## Learned Project Facts\n- New fact\n"
        );
    }

    #[test]
    fn test_format_updated_content_existing_section() {
        let existing =
            "# My Project\n\n## Learned Project Facts\n- Old fact\n\n## Other\n- Something else\n";
        let res = format_updated_content(existing, &["New fact".into()]);
        assert!(res.contains("## Learned Project Facts\n- New fact\n- Old fact"));
        assert!(res.contains("## Other\n- Something else"));
    }
}
