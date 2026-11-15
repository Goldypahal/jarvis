//! Slash commands (`/undo`, `/models`, `/<skill>` ...), shared by every interface.
//! They return plain text instead of printing, so the terminal, the full-screen
//! UI and the web UI can each show it their own way.

use anyhow::Result;
use oblivion_core::{Agent, AgentEvent, ApprovalMode};
use oblivion_mcp::McpHub;
use oblivion_skills::SkillSet;
use oblivion_tools::TodoList;

pub enum Command {
    Quit,
    /// Finished; show this text.
    Done(String),
    /// Run this as a request to the agent.
    Run(String),
}

/// The user message for `/skill-name args`: the skill's instructions are
/// included directly, saving the model a round trip to load them.
pub fn skill_request(skills: &SkillSet, name: &str, arg: &str) -> String {
    let loaded = skills.load_text(name).unwrap_or_default();
    let request = if arg.trim().is_empty() {
        "Follow the skill's instructions."
    } else {
        arg.trim()
    };
    format!("The user ran the /{name} skill. Its instructions are loaded below; follow them.\n\n{loaded}\n\nRequest: {request}")
}

pub const HELP: &str = "\
/undo          revert file changes from the last request
/models        show models, cooldowns and today's usage
/todo          show the agent's task list
/compact       summarize older turns now to free up space
/model <text>  only use models whose label contains <text>
/approval <m>  ask | auto-edit | yolo
/clear         start a fresh conversation
/session       show the session id (resume with --resume <id>)
/skills        list skills; run one with /<skill-name> [request]
/mcp           list MCP servers and their tools
/exit          quit";

pub fn models_text(agent: &Agent) -> String {
    agent
        .router
        .status()
        .into_iter()
        .map(|st| {
            let used = match (st.used_today, st.requests_per_day) {
                (Some(u), Some(d)) => format!(" {u}/{d} today"),
                (Some(u), None) => format!(" {u} today"),
                _ => String::new(),
            };
            match st.cooldown {
                Some(d) => format!("… {} (cooling down {}s){used}", st.label, d.as_secs()),
                None => format!("✓ {}{used}", st.label),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn handle_command(
    agent: &mut Agent,
    skills: &SkillSet,
    hub: &McpHub,
    cmd: &str,
    sink: &mut (dyn FnMut(AgentEvent) + Send),
) -> Result<Command> {
    let (name, arg) = cmd
        .split_once(' ')
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((cmd, ""));
    let text = match name {
        "exit" | "quit" | "q" => return Ok(Command::Quit),
        name if skills.get(name).is_some() => {
            return Ok(Command::Run(skill_request(skills, name, arg)));
        }
        "skills" => {
            if skills.is_empty() {
                "No skills found.".to_string()
            } else {
                skills
                    .all()
                    .map(|sk| {
                        let d: String = sk.description.chars().take(80).collect();
                        format!("/{:<16} {d}", sk.name)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        "mcp" => {
            if hub.is_empty() {
                "No MCP servers configured.".to_string()
            } else {
                let mut out = Vec::new();
                for name in hub.names() {
                    match hub.list_tools(&name).await {
                        Ok(t) => out.push(t),
                        Err(e) => out.push(format!("✗ {name}: {e}")),
                    }
                }
                out.join("\n")
            }
        }
        "help" | "h" => HELP.to_string(),
        "undo" => match agent.undo_last_turn()? {
            Some(r) => format!(
                "Restored {} file(s), removed {} new file(s).",
                r.restored, r.removed
            ),
            None => "Nothing to undo.".to_string(),
        },
        "models" => models_text(agent),
        "todo" => TodoList::render(&agent.tools.todos().items()),
        "compact" => {
            if agent.compact(true, sink).await? {
                "Earlier turns summarized.".to_string()
            } else {
                "Nothing to compact yet.".to_string()
            }
        }
        "model" => {
            if arg.is_empty() {
                "Usage: /model <text>".to_string()
            } else if agent.router.pin(arg) {
                format!(
                    "Using: {}",
                    agent
                        .router
                        .targets()
                        .iter()
                        .map(|t| t.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("No model matches '{arg}'.")
            }
        }
        "approval" => match ApprovalMode::parse(arg) {
            Some(m) => {
                agent.approval = m;
                format!("Approval mode: {}", approval_name(m))
            }
            None => "Usage: /approval ask | auto-edit | yolo".to_string(),
        },
        "clear" => {
            agent.clear();
            "Conversation cleared.".to_string()
        }
        "session" => format!("Session {}", agent.session.id),
        other => format!("Unknown command /{other}. Try /help or /skills."),
    };
    Ok(Command::Done(text))
}

pub fn approval_name(m: ApprovalMode) -> &'static str {
    match m {
        ApprovalMode::Ask => "ask",
        ApprovalMode::AutoEdit => "auto-edit",
        ApprovalMode::Yolo => "yolo",
    }
}
