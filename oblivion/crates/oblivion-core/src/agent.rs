//! The agent loop (SRS FR-01..FR-07): ask the model, run the tools it calls,
//! feed results back, repeat until it answers without calling tools.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use oblivion_providers::{Message, Role, Router, StreamEvent, Usage};
use oblivion_tools::{Risk, ToolBox, UndoReport};
use serde_json::Value;

use crate::compact::{self, Stage};
use crate::config::ApprovalMode;
use crate::rules::{Rules, Verdict};
use crate::session::{JsonDir, Session, SessionStore};

/// What the user said to an approval prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Yes,
    No,
    /// Yes, and stop asking for this tool for the rest of the session.
    Always,
}

/// Asks the user before risky tool calls. The CLI implements this with a prompt.
pub trait Approver: Send + Sync {
    fn approve(&self, tool: &str, summary: &str) -> Decision;
}

/// Progress reported to the interface while a turn runs.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    Text(String),
    /// The model finished a message (text may follow tool calls).
    TextEnd,
    ToolStart {
        name: String,
        summary: String,
    },
    ToolEnd {
        name: String,
        preview: String,
        is_error: bool,
    },
    Declined {
        name: String,
    },
    /// Refused by a deny rule in the config.
    Blocked {
        name: String,
        rule: String,
    },
    Fallback {
        from: String,
        reason: String,
    },
    Model(String),
    Notice(String),
    /// All models are rate-limited for a few seconds; waiting before retrying.
    Waiting(u64),
}

#[derive(Debug, Default, Clone)]
pub struct TurnSummary {
    pub steps: usize,
    pub tool_calls: usize,
    pub usage: Usage,
    pub models: Vec<String>,
    pub hit_step_limit: bool,
}

pub struct Agent {
    pub router: Router,
    pub tools: ToolBox,
    pub approval: ApprovalMode,
    pub max_steps: usize,
    pub session: Session,
    /// Conversation size (estimated tokens) at which history is shrunk.
    pub context_tokens: usize,
    rules: Rules,
    store: Option<Arc<dyn SessionStore>>,
    approver: Box<dyn Approver>,
    always_allowed: HashSet<String>,
    /// Journal marks at the start of each turn, for `/undo`.
    turn_marks: Vec<usize>,
}

impl Agent {
    pub fn new(
        router: Router,
        tools: ToolBox,
        approver: Box<dyn Approver>,
        approval: ApprovalMode,
        max_steps: usize,
        system_prompt: String,
    ) -> Self {
        let mut session = Session::new(tools.root().to_path_buf());
        session.messages.push(Message::system(system_prompt));
        Self {
            router,
            tools,
            approval,
            max_steps,
            session,
            context_tokens: 24_000,
            rules: Rules::default(),
            store: None,
            approver,
            always_allowed: HashSet::new(),
            turn_marks: Vec::new(),
        }
    }

    /// Save the session as JSON files in `dir` after every step.
    pub fn persist_to(&mut self, dir: PathBuf) {
        self.store = Some(Arc::new(JsonDir(dir)));
    }

    /// Save the session to `store` after every step.
    pub fn persist_with(&mut self, store: Arc<dyn SessionStore>) {
        self.store = Some(store);
    }

    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
    }

    /// Continue an earlier conversation, keeping this run's fresh system prompt.
    pub fn resume(&mut self, mut old: Session) {
        let system = self.session.messages.first().cloned();
        old.messages.retain(|m| m.role != Role::System);
        if let Some(s) = system {
            old.messages.insert(0, s);
        }
        self.session = old;
        self.repair();
    }

    /// Forget the conversation but keep the system prompt.
    pub fn clear(&mut self) {
        self.session.messages.truncate(1);
    }

    fn save(&self) {
        if let Some(store) = &self.store {
            // Losing a save should not kill the task; the next step saves again.
            let _ = store.save(&self.session);
        }
    }

    /// The budget compaction aims for: the configured size, but never more than
    /// the biggest request any available model accepts.
    fn budget(&self) -> usize {
        match self.router.largest_input_budget() {
            Some(max) => self.context_tokens.min(max as usize),
            None => self.context_tokens,
        }
    }

    /// Shrink history if it is near the budget (SRS FR-51). `force` summarizes
    /// even when under budget (the /compact command).
    pub async fn compact(
        &mut self,
        force: bool,
        on_event: &mut (dyn FnMut(AgentEvent) + Send),
    ) -> Result<bool> {
        let specs = self.tools.specs();
        let budget = self.budget();
        let stage = if force {
            Stage::NeedsSummary
        } else {
            compact::trim_old_tool_output(&mut self.session.messages, &specs, budget)
        };
        if stage != Stage::NeedsSummary {
            return Ok(stage == Stage::Trimmed);
        }
        let Some(split) = compact::split_point(&self.session.messages, &specs, budget) else {
            return Ok(false);
        };
        if split <= 1 {
            return Ok(false);
        }
        on_event(AgentEvent::Notice(
            "Conversation is long; summarizing earlier turns to save space…".into(),
        ));
        let text = compact::transcript(&self.session.messages[1..split], budget * 2);
        let request = vec![
            Message::system(compact::SUMMARY_PROMPT),
            Message::user(text),
        ];
        let mut quiet = |e: StreamEvent| match e {
            StreamEvent::Fallback { from, reason } => {
                on_event(AgentEvent::Fallback { from, reason })
            }
            StreamEvent::Waiting { seconds } => on_event(AgentEvent::Waiting(seconds)),
            StreamEvent::TextDelta(_) => {}
        };
        let summary = match self.router.complete(&request, &[], &mut quiet).await {
            Ok(c) => c.message.content.unwrap_or_default(),
            Err(_) => String::new(),
        };
        let summary = if summary.trim().is_empty() {
            "(Earlier messages were removed to save space; no summary was available.)".to_string()
        } else {
            summary
        };
        compact::apply_summary(&mut self.session.messages, split, &summary);
        self.save();
        Ok(true)
    }

    /// Undo every file change from the most recent turn.
    pub fn undo_last_turn(&mut self) -> Result<Option<UndoReport>> {
        match self.turn_marks.pop() {
            Some(mark) => Ok(Some(self.tools.journal().undo_to(mark)?)),
            None => Ok(None),
        }
    }

    /// After a cancelled turn, give every unanswered tool call a result so the
    /// history stays valid for the next request.
    pub fn repair(&mut self) {
        let mut answered: HashSet<String> = HashSet::new();
        for m in &self.session.messages {
            if let Some(id) = &m.tool_call_id {
                answered.insert(id.clone());
            }
        }
        let mut fixes = Vec::new();
        for (i, m) in self.session.messages.iter().enumerate() {
            for c in &m.tool_calls {
                if !answered.contains(&c.id) {
                    fixes.push((i, c.id.clone()));
                }
            }
        }
        // Insert right after the assistant message, in reverse to keep indices valid.
        for (i, id) in fixes.into_iter().rev() {
            let mut at = i + 1;
            while at < self.session.messages.len() && self.session.messages[at].role == Role::Tool {
                at += 1;
            }
            self.session.messages.insert(
                at,
                Message::tool_result(id, "Cancelled by the user before it ran."),
            );
        }
        self.save();
    }

    fn needs_approval(&self, name: &str, risk: &Risk) -> Option<String> {
        let Risk::Ask(summary) = risk else {
            return None;
        };
        if self.always_allowed.contains(name) {
            return None;
        }
        match self.approval {
            ApprovalMode::Yolo => None,
            ApprovalMode::AutoEdit => {
                let edits_inside = matches!(name, "write_file" | "edit_file")
                    && !summary.contains("outside the workspace");
                if edits_inside {
                    None
                } else {
                    Some(summary.clone())
                }
            }
            ApprovalMode::Ask => Some(summary.clone()),
        }
    }

    /// Run one user request to completion.
    pub async fn run_turn(
        &mut self,
        user_text: &str,
        on_event: &mut (dyn FnMut(AgentEvent) + Send),
    ) -> Result<TurnSummary> {
        self.turn_marks.push(self.tools.journal().mark());
        self.session.messages.push(Message::user(user_text));
        self.save();

        let specs = self.tools.specs();
        let mut summary = TurnSummary::default();

        loop {
            if summary.steps >= self.max_steps {
                summary.hit_step_limit = true;
                on_event(AgentEvent::Notice(format!(
                    "Stopped after {} steps (agent.max_steps). Say \"continue\" to keep going.",
                    self.max_steps
                )));
                break;
            }
            summary.steps += 1;
            self.compact(false, on_event).await?;

            let completion = {
                let mut forward = |e: StreamEvent| match e {
                    StreamEvent::TextDelta(t) => on_event(AgentEvent::Text(t)),
                    StreamEvent::Fallback { from, reason } => {
                        on_event(AgentEvent::Fallback { from, reason })
                    }
                    StreamEvent::Waiting { seconds } => on_event(AgentEvent::Waiting(seconds)),
                };
                self.router
                    .complete(&self.session.messages, &specs, &mut forward)
                    .await?
            };
            if completion.message.content.is_some() {
                on_event(AgentEvent::TextEnd);
            }
            if summary.models.last() != Some(&completion.served_by) {
                on_event(AgentEvent::Model(completion.served_by.clone()));
                summary.models.push(completion.served_by.clone());
            }
            if let Some(u) = &completion.usage {
                summary.usage.prompt_tokens += u.prompt_tokens;
                summary.usage.completion_tokens += u.completion_tokens;
            }

            let mut msg = completion.message;
            if msg.content.is_none() && msg.tool_calls.is_empty() {
                // Empty reply: store something valid so the history can be sent again.
                msg.content = Some(String::new());
                on_event(AgentEvent::Notice(
                    "The model returned an empty reply.".into(),
                ));
            }
            let calls = msg.tool_calls.clone();
            self.session.messages.push(msg);
            self.save();

            if calls.is_empty() {
                break;
            }

            for call in calls {
                summary.tool_calls += 1;
                let name = call.function.name.clone();
                let args: Value = match serde_json::from_str(&call.function.arguments) {
                    Ok(v @ Value::Object(_)) => v,
                    Ok(_) | Err(_) => {
                        let msg = format!(
                            "Error: the arguments for {name} were not a valid JSON object: {}. \
                             Call the tool again with valid JSON.",
                            truncate(&call.function.arguments, 300)
                        );
                        on_event(AgentEvent::ToolEnd {
                            name: name.clone(),
                            preview: "invalid JSON arguments".into(),
                            is_error: true,
                        });
                        self.session
                            .messages
                            .push(Message::tool_result(&call.id, msg));
                        continue;
                    }
                };

                let risk = self.tools.risk(&name, &args);
                let label = match &risk {
                    Risk::Ask(s) => s.clone(),
                    Risk::Safe => describe(&name, &args),
                };
                let verdict = self.rules.check(&name, &self.tools.subject(&name, &args));
                if let Verdict::Deny(rule) = &verdict {
                    on_event(AgentEvent::Blocked {
                        name: name.clone(),
                        rule: rule.clone(),
                    });
                    self.session.messages.push(Message::tool_result(
                        &call.id,
                        format!("Blocked by the rule `{rule}` in oblivion.toml. Do not try to work around it; find another way or ask the user."),
                    ));
                    continue;
                }
                let pre_approved = matches!(verdict, Verdict::Allow(_));
                if let Some(question) = self.needs_approval(&name, &risk).filter(|_| !pre_approved)
                {
                    let decision =
                        tokio::task::block_in_place(|| self.approver.approve(&name, &question));
                    match decision {
                        Decision::No => {
                            on_event(AgentEvent::Declined { name: name.clone() });
                            self.session.messages.push(Message::tool_result(
                                &call.id,
                                "The user declined this action. Do not retry it; ask how they want to proceed.",
                            ));
                            continue;
                        }
                        Decision::Always => {
                            self.always_allowed.insert(name.clone());
                        }
                        Decision::Yes => {}
                    }
                }

                on_event(AgentEvent::ToolStart {
                    name: name.clone(),
                    summary: label,
                });
                let out = self.tools.execute(&name, &args).await;
                on_event(AgentEvent::ToolEnd {
                    name: name.clone(),
                    preview: first_lines(&out.text, 3),
                    is_error: out.is_error,
                });
                self.session
                    .messages
                    .push(Message::tool_result(&call.id, out.text));
                self.save();
            }
        }
        Ok(summary)
    }
}

fn describe(name: &str, args: &Value) -> String {
    let get = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string()
    };
    match name {
        "read_file" => format!("read {}", get("path")),
        "glob" => format!("find {}", get("pattern")),
        "grep" => format!("search /{}/", get("pattern")),
        "web_search" => format!("search the web: {}", get("query")),
        "todo" => "update task list".into(),
        _ => name.to_string(),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

fn first_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().take(n).collect();
    let more = s.lines().count().saturating_sub(n);
    let mut out = lines.join("\n");
    if more > 0 {
        out.push_str(&format!("\n… ({more} more lines)"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use oblivion_providers::{FunctionCall, ToolCall};

    struct Never;
    impl Approver for Never {
        fn approve(&self, _: &str, _: &str) -> Decision {
            Decision::No
        }
    }

    fn agent(dir: &std::path::Path) -> Agent {
        let tools = ToolBox::new(dir, oblivion_tools::Shell::Sh, dir.join(".snap"));
        Agent::new(
            Router::new(vec![]),
            tools,
            Box::new(Never),
            ApprovalMode::Ask,
            5,
            "sys".into(),
        )
    }

    #[test]
    fn repair_answers_dangling_calls() {
        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        a.session.messages.push(Message::user("hi"));
        let call = |id: &str| ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "shell".into(),
                arguments: "{}".into(),
            },
        };
        a.session
            .messages
            .push(Message::assistant(None, vec![call("a"), call("b")]));
        a.session.messages.push(Message::tool_result("a", "done"));
        a.repair();
        let roles: Vec<Role> = a.session.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            vec![
                Role::System,
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::Tool
            ]
        );
        assert_eq!(a.session.messages[4].tool_call_id.as_deref(), Some("b"));
    }

    #[test]
    fn approval_modes() {
        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        let edit = Risk::Ask("edit x".into());
        let outside = Risk::Ask("write /x (outside the workspace)".into());
        let cmd = Risk::Ask("run: ls".into());
        assert!(a.needs_approval("edit_file", &edit).is_some());
        a.approval = ApprovalMode::AutoEdit;
        assert!(a.needs_approval("edit_file", &edit).is_none());
        assert!(a.needs_approval("write_file", &outside).is_some());
        assert!(a.needs_approval("shell", &cmd).is_some());
        a.approval = ApprovalMode::Yolo;
        assert!(a.needs_approval("shell", &cmd).is_none());
        assert!(a.needs_approval("read_file", &Risk::Safe).is_none());
    }
}
