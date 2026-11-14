//! Sub-agents (SRS FR-18): the `task` tool starts a fresh agent with its own
//! short conversation, gives it a job, and returns only its final report.
//!
//! Why: small free models do much better on one narrow job with a clean
//! context than on a long, crowded conversation. Several `task` calls in one
//! turn run in parallel.
//!
//! Safety: sub-agents are read-only. They can read and search the workspace,
//! search and fetch web pages, and load skills; they cannot edit files, run
//! commands or call MCP tools. Reading outside the workspace is refused, so a
//! malicious web page cannot make a sub-agent read your secrets.

use std::path::PathBuf;
use std::sync::Arc;

use oblivion_providers::{Router, ToolSpec};
use oblivion_tools::{SearchBackend, Shell, ToolBox, ToolProvider};
use serde_json::json;

use crate::agent::{Agent, AgentEvent, Approver, Decision};
use crate::config::ApprovalMode;
use crate::rules::Rules;

/// Tools a sub-agent may use.
pub const SUBAGENT_TOOLS: &[&str] = &[
    "read_file",
    "glob",
    "grep",
    "web_search",
    "web_fetch",
    "skill",
];
const MAX_REPORT_CHARS: usize = 12_000;

/// Web searches + fetches each sub-agent may make: enough for one sub-question.
pub const SUBAGENT_WEB_CALLS: usize = 8;

/// Allows only web_fetch (read-only network access); everything else that
/// would need approval, such as reading outside the workspace, is refused.
struct SubagentApprover;

impl Approver for SubagentApprover {
    fn approve(&self, tool: &str, _summary: &str) -> Decision {
        if tool == "web_fetch" {
            Decision::Yes
        } else {
            Decision::No
        }
    }
}

/// Everything needed to start sub-agents like the main agent.
pub struct SubagentFactory {
    pub router: Router,
    pub root: PathBuf,
    pub shell: Shell,
    pub search: SearchBackend,
    pub skills: Option<Arc<dyn ToolProvider>>,
    /// Folders readable without approval besides the workspace (skill folders).
    pub read_roots: Vec<PathBuf>,
    /// The main agent's system prompt (environment, skills list, project rules).
    pub base_prompt: String,
    pub context_tokens: usize,
    pub max_steps: usize,
    pub rules: Rules,
    /// Model filter tried first for sub-agents (e.g. "ollama"); empty = normal order.
    pub prefer: String,
}

pub fn task_spec() -> ToolSpec {
    ToolSpec {
        name: "task".into(),
        description: "Start a read-only sub-agent for one focused job (research a question on the web, \
            find where something is implemented, summarize files). It has its own short context and returns \
            a report. Put several task calls in ONE response to run them in parallel. Sub-agents can read, \
            search and fetch, but cannot edit files or run commands, so do those yourself afterwards. Give \
            each a complete, self-contained prompt: it cannot see this conversation."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "description": {"type": "string", "description": "3-6 word label, e.g. 'Find auth middleware'"},
                "prompt": {"type": "string", "description": "Full instructions, including what the report must contain"},
                "model": {"type": "string", "description": "Optional model filter, e.g. 'ollama' or 'groq'"}
            },
            "required": ["description", "prompt"]
        }),
    }
}

impl SubagentFactory {
    fn system_prompt(&self) -> String {
        format!(
            "{}\n\nYou are a sub-agent. Another agent gave you one job; it sees only your final reply.\n\
             - You can read and search files in the workspace, search and fetch web pages, and load skills.\n\
             - You cannot edit files or run commands. Do not ask the user questions; work with what you have.\n\
             - Be efficient: a few targeted searches and reads, then answer.\n\
             - Final reply: a concise report with the facts found, file paths with line numbers, and source URLs\n\
               for web facts. Say clearly what you could not find.",
            self.base_prompt
        )
    }

    pub fn spawn(&self, model: Option<&str>) -> Agent {
        let snap = std::env::temp_dir().join("oblivion-subagent-snapshots");
        let mut tools = ToolBox::new(self.root.clone(), self.shell, snap)
            .with_search(self.search.clone())
            .restrict_to(SUBAGENT_TOOLS)
            .allow_reads_in(self.read_roots.clone());
        if let Some(s) = &self.skills {
            tools.add_provider(s.clone());
        }
        let mut router = self.router.share().preferring(&self.prefer);
        if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
            // A filter that matches nothing keeps the full list rather than failing.
            let mut pinned = router.share();
            if pinned.pin(m.trim()) {
                router = pinned;
            }
        }
        let mut agent = Agent::new(
            router,
            tools,
            Box::new(SubagentApprover),
            ApprovalMode::Ask,
            self.max_steps,
            self.system_prompt(),
        );
        agent.context_tokens = self.context_tokens;
        agent.max_web_calls = SUBAGENT_WEB_CALLS;
        agent.set_rules(self.rules.clone());
        agent
    }

    /// Run one sub-agent job and return its report.
    pub async fn run(
        &self,
        prompt: &str,
        model: Option<&str>,
        on_event: &mut (dyn FnMut(AgentEvent) + Send),
    ) -> Result<String, String> {
        let mut agent = self.spawn(model);
        let summary = agent
            .run_turn(prompt, on_event)
            .await
            .map_err(|e| format!("sub-agent failed: {e:#}"))?;
        let report = agent
            .session
            .messages
            .iter()
            .rev()
            .find(|m| {
                m.role == oblivion_providers::Role::Assistant
                    && m.content.as_deref().is_some_and(|c| !c.trim().is_empty())
            })
            .and_then(|m| m.content.clone())
            .unwrap_or_else(|| "(the sub-agent finished without a report)".into());
        let mut report: String = if report.chars().count() > MAX_REPORT_CHARS {
            report.chars().take(MAX_REPORT_CHARS).collect::<String>() + "\n[report truncated]"
        } else {
            report
        };
        if summary.hit_step_limit {
            report.push_str(
                "\n[note: the sub-agent hit its step limit; the report may be incomplete]",
            );
        }
        Ok(report)
    }
}
