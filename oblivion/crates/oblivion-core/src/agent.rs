//! The agent loop (SRS FR-01..FR-07): ask the model, run the tools it calls,
//! feed results back, repeat until it answers without calling tools.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use oblivion_providers::{Message, Role, Router, StreamEvent, ToolSpec, Usage};
use oblivion_tools::{Risk, ToolBox, UndoReport};
use serde_json::Value;

use crate::compact::{self, Stage};
use crate::config::ApprovalMode;
use crate::rules::{Rules, Verdict};
use crate::session::{JsonDir, Session, SessionStore};
use crate::subagent::{task_spec, SubagentFactory};

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
        /// What the call will change, for interfaces that show it: removed/added
        /// lines for edit_file, the content for write_file, the command for shell.
        detail: String,
    },
    ToolEnd {
        name: String,
        preview: String,
        is_error: bool,
        /// The result text (capped), for interfaces that can expand it.
        output: String,
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
    /// Progress from sub-agent number `index` (labelled with its description).
    Sub {
        index: usize,
        label: String,
        event: Box<AgentEvent>,
    },
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
    /// Model filter tried first for conversation summaries (e.g. "ollama").
    pub summary_prefer: String,
    /// web_search + web_fetch calls allowed per request before the model is told
    /// to answer with what it has (free tiers allow few tokens per day).
    pub max_web_calls: usize,
    /// Whether the end-of-session memory extraction pass runs.
    pub memory_auto_extract: bool,
    /// Maximum tokens budgeted for the memory extraction pass.
    pub memory_extract_tokens: usize,
    pub memory_recall: Option<Arc<dyn crate::extract::MemoryRecall>>,
    /// When true, only local models are used and cloud providers are purged.
    pub offline: bool,
    /// Model priority patterns for each request class.
    pub routing_rules: crate::routing::RoutingRules,
    /// When true, enables small-model mode (shorter prompt, core tools, strict repair).
    pub small_model: bool,
    /// When true, enables spoken reply style (short conversational answers, code omitted from speech).
    pub spoken_reply: bool,
    /// When true, enables hands-free wake word listening.
    pub wake_word_enabled: bool,
    /// Currently active or pinned model label.
    pub active_model: Option<String>,
    /// Compact system prompt used when small-model mode is active.
    pub small_system_prompt: Option<String>,
    base_system_prompt: String,
    rules: Rules,
    store: Option<Arc<dyn SessionStore>>,
    approver: Box<dyn Approver>,
    always_allowed: HashSet<String>,
    /// Journal marks at the start of each turn, for `/undo`.
    turn_marks: Vec<usize>,
    subagents: Option<Arc<SubagentFactory>>,
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
        session
            .messages
            .push(Message::system(system_prompt.clone()));
        Self {
            router,
            tools,
            approval,
            max_steps,
            session,
            context_tokens: 24_000,
            summary_prefer: String::new(),
            max_web_calls: 15,
            memory_auto_extract: true,
            memory_extract_tokens: 2000,
            memory_recall: None,
            offline: false,
            routing_rules: crate::routing::RoutingRules::default(),
            small_model: false,
            spoken_reply: false,
            wake_word_enabled: false,
            active_model: None,
            small_system_prompt: None,
            base_system_prompt: system_prompt,
            rules: Rules::default(),
            store: None,
            approver,
            always_allowed: HashSet::new(),
            turn_marks: Vec::new(),
            subagents: None,
        }
    }

    /// Provide a memory recall implementation to inject relevant memories into turns.
    pub fn set_memory_recall(&mut self, recall: Arc<dyn crate::extract::MemoryRecall>) {
        self.memory_recall = Some(recall);
    }

    /// Save the session as JSON files in `dir` after every step.
    pub fn persist_to(&mut self, dir: PathBuf) {
        self.store = Some(Arc::new(JsonDir(dir)));
    }

    /// Save the session to `store` after every step.
    pub fn persist_with(&mut self, store: Arc<dyn SessionStore>) {
        self.store = Some(store);
    }

    /// Enable the `task` tool (sub-agents).
    pub fn set_subagents(&mut self, factory: SubagentFactory) {
        self.subagents = Some(Arc::new(factory));
    }

    /// Check whether small-model mode is explicitly enabled or automatically active.
    pub fn is_small_model_active(&self) -> bool {
        if self.small_model {
            return true;
        }
        if let Some(ref m) = self.active_model {
            crate::small_model::is_small_model_name(m)
        } else {
            false
        }
    }

    fn all_specs(&self) -> Vec<oblivion_providers::ToolSpec> {
        let mut specs = self.tools.specs();
        if self.subagents.is_some() && !self.is_small_model_active() {
            specs.push(task_spec());
        }
        if self.is_small_model_active() {
            const CORE_TOOLS: &[&str] = &[
                "read_file",
                "write_file",
                "edit_file",
                "glob",
                "grep",
                "shell",
                "todo",
            ];
            specs.retain(|s| CORE_TOOLS.contains(&s.name.as_str()));
        }
        specs
    }

    /// Run every `task` call of one model turn in parallel, forwarding their
    /// progress as it happens. Returns call id -> tool result text.
    async fn run_tasks(
        &self,
        calls: &[oblivion_providers::ToolCall],
        on_event: &mut (dyn FnMut(AgentEvent) + Send),
    ) -> std::collections::HashMap<String, String> {
        use futures_util::future::join_all;
        let mut results = std::collections::HashMap::new();
        let Some(factory) = self.subagents.clone() else {
            return results;
        };
        let jobs: Vec<(usize, String, String, String, Option<String>)> = calls
            .iter()
            .filter(|c| c.function.name == "task")
            .enumerate()
            .filter_map(|(i, c)| {
                let v: Value = serde_json::from_str(&c.function.arguments).ok()?;
                let prompt = v.get("prompt").and_then(Value::as_str)?.to_string();
                if let Verdict::Deny(_) = self.rules.check("task", &prompt) {
                    return None; // handled as blocked by the normal loop
                }
                let label = v
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("sub-agent")
                    .to_string();
                let model = v.get("model").and_then(Value::as_str).map(str::to_string);
                Some((i + 1, c.id.clone(), label, prompt, model))
            })
            .collect();
        if jobs.is_empty() {
            return results;
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();
        let futures = jobs.into_iter().map(|(index, id, label, prompt, model)| {
            let tx = tx.clone();
            let factory = factory.clone();
            async move {
                let _ = tx.send(AgentEvent::Sub {
                    index,
                    label: label.clone(),
                    event: Box::new(AgentEvent::Notice("started".into())),
                });
                let mut forward = |e: AgentEvent| {
                    // Streaming text of sub-agents would interleave; send the rest.
                    if !matches!(e, AgentEvent::Text(_) | AgentEvent::TextEnd) {
                        let _ = tx.send(AgentEvent::Sub {
                            index,
                            label: label.clone(),
                            event: Box::new(e),
                        });
                    }
                };
                let out = factory.run(&prompt, model.as_deref(), &mut forward).await;
                let text = match out {
                    Ok(report) => format!("Report from sub-agent \"{label}\":\n{report}"),
                    Err(e) => format!("Error: sub-agent \"{label}\" failed: {e}"),
                };
                let _ = tx.send(AgentEvent::Sub {
                    index,
                    label,
                    event: Box::new(AgentEvent::Notice("finished".into())),
                });
                (id, text)
            }
        });
        let futures: Vec<_> = futures.collect();
        drop(tx);
        let all = join_all(futures);
        tokio::pin!(all);
        let done = loop {
            tokio::select! {
                done = &mut all => break done,
                Some(e) = rx.recv() => on_event(e),
            }
        };
        while let Ok(e) = rx.try_recv() {
            on_event(e);
        }
        results.extend(done);
        results
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
        self.tools.set_root(old.workspace.clone());
        if let Some(sub) = &self.subagents {
            *sub.root.write().unwrap() = old.workspace.clone();
        }
        self.session = old;
        self.repair();
    }

    /// Set a new workspace folder for the session and tools.
    pub fn set_workspace(&mut self, new_workspace: std::path::PathBuf) {
        self.session.workspace = new_workspace.clone();
        self.tools.set_root(new_workspace.clone());
        if let Some(sub) = &self.subagents {
            *sub.root.write().unwrap() = new_workspace;
        }
    }

    /// Start a new session with the given workspace.
    pub fn new_session(&mut self, workspace: std::path::PathBuf) {
        let system = self.session.messages.first().cloned();
        let mut s = Session::new(workspace.clone());
        if let Some(sys) = system {
            s.messages.push(sys);
        }
        self.tools.set_root(workspace.clone());
        if let Some(sub) = &self.subagents {
            *sub.root.write().unwrap() = workspace;
        }
        self.session = s;
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
        let specs = self.all_specs();
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
        let mut summarizer = self.router.share().preferring(&self.summary_prefer);
        let summary = match summarizer.complete(&request, &[], &mut quiet).await {
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

    /// Run the end-of-session memory extraction pass to propose durable facts and preferences.
    /// Uses a budgeted cheap model call, falling back to heuristic extraction.
    /// Returns an empty list if `memory_auto_extract` is false or if no user interaction occurred.
    pub async fn extract_end_of_session_memories(&self) -> Vec<crate::extract::ProposedMemory> {
        if !self.memory_auto_extract || self.session.messages.len() < 2 {
            return Vec::new();
        }
        let budget_chars = self.memory_extract_tokens * 4;
        let mut on_fb = |_from, _reason| {};
        crate::extract::extract_memories(
            &self.router,
            &self.summary_prefer,
            &self.session.messages,
            budget_chars,
            &mut on_fb,
        )
        .await
    }

    /// Check if any extracted memories warrant a proposed update to AGENTS.md.
    pub fn propose_agents_md_update(
        &self,
        memories: &[crate::extract::ProposedMemory],
    ) -> Option<crate::agents_md::AgentsMdProposal> {
        crate::agents_md::propose_update(self.tools.root(), memories)
    }

    /// Apply an AGENTS.md proposal to disk and records a snapshot in the undo journal.
    pub fn apply_agents_md_update(
        &self,
        proposal: &crate::agents_md::AgentsMdProposal,
    ) -> std::io::Result<()> {
        let _ = self.tools.journal().snapshot(&proposal.path);
        crate::agents_md::apply_update(proposal)
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
        self.run_turn_with_images(user_text, vec![], on_event).await
    }

    /// Run one user request to completion, optionally providing attached images for visual reasoning.
    pub async fn run_turn_with_images(
        &mut self,
        user_text: &str,
        images: Vec<oblivion_providers::ImageAttachment>,
        on_event: &mut (dyn FnMut(AgentEvent) + Send),
    ) -> Result<TurnSummary> {
        self.turn_marks.push(self.tools.journal().mark());
        let mut effective_system = self.base_system_prompt.clone();
        if let Some(ref recall) = self.memory_recall {
            let project = self.tools.root().to_string_lossy();
            let relevant = recall.recall_for_prompt(user_text, &project, 2000).await;
            if !relevant.is_empty() {
                let mem_lines = relevant
                    .iter()
                    .map(|m| format!("- {m}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                effective_system = format!(
                    "{}\n\nRelevant memories from earlier sessions:\n<relevant_memories>\n{}\n</relevant_memories>",
                    effective_system, mem_lines
                );
            }
        }
        if self.spoken_reply {
            effective_system = crate::prompt::append_spoken_instructions(&effective_system);
        }
        if !self.session.messages.is_empty() && self.session.messages[0].role == Role::System {
            self.session.messages[0].content = Some(effective_system);
        }
        let has_images = !images.is_empty();
        self.session
            .messages
            .push(Message::user_with_images(user_text, images));
        self.save();

        let history_tokens = self.budget();
        let class =
            crate::routing::classify_request(user_text, history_tokens, has_images, self.offline);
        let patterns = self.routing_rules.patterns_for(class);
        let offline_or_private = self.offline || class == crate::routing::RequestClass::Private;
        let mut turn_router = self
            .router
            .share()
            .for_patterns(patterns, offline_or_private);

        if self.is_small_model_active() {
            if let Some(ref small_prompt) = self.small_system_prompt {
                if !self.session.messages.is_empty()
                    && self.session.messages[0].role == Role::System
                {
                    let final_small = if self.spoken_reply {
                        crate::prompt::append_spoken_instructions(small_prompt)
                    } else {
                        small_prompt.clone()
                    };
                    self.session.messages[0].content = Some(final_small);
                }
            }
        }

        let specs = self.all_specs();
        let mut summary = TurnSummary::default();
        // Loop guard: free models sometimes forget a result and repeat the exact
        // same call; refuse the third identical call and stop if it keeps happening.
        let mut repeats: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut refused_repeats = 0usize;
        let mut stuck = false;
        let mut web_calls = 0usize;
        // Fingerprint of the latest browser page: clicking "Next" (same ref) on a
        // new page is progress, not a repeat.
        let mut page_hash = 0u64;
        // Steps before the end at which the model is told to wrap up.
        let warn_at = if self.max_steps > 8 {
            self.max_steps - 4
        } else {
            usize::MAX
        };

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
            let last_step = summary.steps == self.max_steps;
            if summary.steps == warn_at {
                self.session.messages.push(Message::user(format!(
                    "[Oblivion] Only {} steps are left for this request. Stop gathering information. \
                     Finish any file you are writing, then give your final answer with what you already have.",
                    self.max_steps - warn_at + 1
                )));
            } else if last_step {
                self.session.messages.push(Message::user(
                    "[Oblivion] This is the last step: tools are switched off. Write your final answer now from \
                     what you have gathered, and say briefly what is still missing.",
                ));
            }
            self.compact(false, on_event).await?;

            let completion = {
                let mut forward = |e: StreamEvent| match e {
                    StreamEvent::TextDelta(t) => on_event(AgentEvent::Text(t)),
                    StreamEvent::Fallback { from, reason } => {
                        on_event(AgentEvent::Fallback { from, reason })
                    }
                    StreamEvent::Waiting { seconds } => on_event(AgentEvent::Waiting(seconds)),
                };
                let tools: &[ToolSpec] = if last_step { &[] } else { &specs };
                match turn_router
                    .complete(&self.session.messages, tools, &mut forward)
                    .await
                {
                    Ok(c) => c,
                    Err(e) if last_step => {
                        summary.hit_step_limit = true;
                        on_event(AgentEvent::Notice(format!(
                            "Stopped after {} steps (agent.max_steps); the final summary request failed: {e}. \
                             Say \"continue\" to keep going.",
                            self.max_steps
                        )));
                        break;
                    }
                    Err(e) => return Err(e.into()),
                }
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
            let mut calls = msg.tool_calls.clone();
            if calls.is_empty() && self.is_small_model_active() {
                if let Some(ref text_content) = msg.content {
                    let valid_names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
                    let extracted = crate::small_model::extract_tool_calls_from_text(
                        text_content,
                        &valid_names,
                    );
                    if !extracted.is_empty() {
                        calls = extracted.clone();
                        msg.tool_calls = extracted;
                    }
                }
            }
            self.session.messages.push(msg);
            self.save();

            if calls.is_empty() {
                break;
            }
            if last_step {
                // Some models still emit tool calls; answer them so the history stays valid.
                for call in &calls {
                    self.session.messages.push(Message::tool_result(
                        &call.id,
                        "Not run: the step limit for this request was reached.",
                    ));
                }
                summary.hit_step_limit = true;
                on_event(AgentEvent::Notice(format!(
                    "Stopped after {} steps (agent.max_steps). Say \"continue\" to keep going.",
                    self.max_steps
                )));
                break;
            }

            let mut task_results = self.run_tasks(&calls, on_event).await;

            for call in calls {
                summary.tool_calls += 1;
                if let Some(text) = task_results.remove(&call.id) {
                    on_event(AgentEvent::ToolEnd {
                        name: "task".into(),
                        preview: first_lines(&text, 2),
                        is_error: text.starts_with("Error:"),
                        output: truncate(&text, EVENT_OUTPUT_CHARS),
                    });
                    self.session
                        .messages
                        .push(Message::tool_result(&call.id, text));
                    self.save();
                    continue;
                }
                let name = call.function.name.clone();
                let args: Value =
                    match crate::small_model::repair_tool_arguments(&call.function.arguments) {
                        Some(v @ Value::Object(_)) => v,
                        _ => {
                            let msg = format!(
                            "Error: the arguments for {name} were not a valid JSON object: {}. \
                             Call the tool again with valid JSON.",
                            truncate(&call.function.arguments, 300)
                        );
                            on_event(AgentEvent::ToolEnd {
                                name: name.clone(),
                                preview: "invalid JSON arguments".into(),
                                is_error: true,
                                output: msg.clone(),
                            });
                            self.session
                                .messages
                                .push(Message::tool_result(&call.id, msg));
                            continue;
                        }
                    };

                if name == "web_search" || name == "web_fetch" {
                    web_calls += 1;
                    if web_calls > self.max_web_calls {
                        on_event(AgentEvent::Notice(format!(
                            "  ⊘ skipped {name}: web budget for this request used ({} calls)",
                            self.max_web_calls
                        )));
                        self.session.messages.push(Message::tool_result(
                            &call.id,
                            format!(
                                "Error: the web budget for this request ({} searches/fetches) is used up. Do not search \
                                 or fetch again. Write your answer now from the pages you already have, cite them, \
                                 and list what is still unknown.",
                                self.max_web_calls
                            ),
                        ));
                        continue;
                    }
                }
                if name != "todo" {
                    let mut key = repeat_key(&name, &args);
                    if name == "browser" {
                        key.push_str(&format!("#{page_hash}"));
                    }
                    let n = repeats.entry(key).or_insert(0);
                    *n += 1;
                    if *n >= 3 {
                        refused_repeats += 1;
                        stuck = refused_repeats >= 3;
                        on_event(AgentEvent::Notice(format!(
                            "  ↺ skipped a repeated {name} call (same arguments {} times)",
                            *n
                        )));
                        self.session.messages.push(Message::tool_result(
                            &call.id,
                            format!(
                                "Error: you already made this exact {name} call {} times in this request and its result has not \
                                 changed. Do not repeat it. Use the earlier result, try a different approach, or give your final \
                                 answer now with what you have.",
                                *n - 1
                            ),
                        ));
                        continue;
                    }
                }

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
                // Announce the call first, so an interface can show the change
                // (diff, command) while the user decides whether to allow it.
                on_event(AgentEvent::ToolStart {
                    name: name.clone(),
                    summary: label,
                    detail: change_detail(&name, &args),
                });
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

                let out = self.tools.execute(&name, &args).await;
                on_event(AgentEvent::ToolEnd {
                    name: name.clone(),
                    preview: first_lines(&out.text, 3),
                    is_error: out.is_error,
                    output: truncate(&out.text, EVENT_OUTPUT_CHARS),
                });
                // Keep any single result to about a quarter of the request budget, so
                // one big web page or log cannot crowd out the rest of the conversation.
                let cap = (self.budget()).max(2_000);
                let text = oblivion_tools::truncate_middle(&out.text, cap);
                if is_page_view(&text) {
                    self.forget_old_pages();
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut h);
                    page_hash = h.finish();
                }
                self.session
                    .messages
                    .push(Message::tool_result(&call.id, text));
                self.save();
            }
            if stuck {
                on_event(AgentEvent::Notice(
                    "Stopped: the model kept repeating the same actions. Try rephrasing the request, \
                     adding another provider (more context), or /compact."
                        .into(),
                ));
                break;
            }
        }
        Ok(summary)
    }
}

/// A browser page outline (many element refs). Only the latest one is useful.
fn is_page_view(text: &str) -> bool {
    text.matches("[ref=").count() >= 3
}

impl Agent {
    /// Replace earlier page outlines with a stub: each browser action returns the
    /// whole page, and old copies would fill a small free-tier request quickly.
    fn forget_old_pages(&mut self) {
        for m in self.session.messages.iter_mut() {
            if m.role == Role::Tool && m.content.as_deref().is_some_and(is_page_view) {
                m.content = Some(
                    "[older page view removed to save space; the newest page view is further down]"
                        .into(),
                );
            }
        }
    }
}

/// Key for the repeat guard. URLs that differ only in query string, fragment or a
/// trailing slash count as the same page; search queries ignore case and spacing.
fn repeat_key(name: &str, args: &Value) -> String {
    let get = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
    match name {
        "web_fetch" => {
            let url = get("url");
            let base = url.split(['?', '#']).next().unwrap_or(url);
            format!("web_fetch:{}", base.trim_end_matches('/').to_lowercase())
        }
        "web_search" => format!(
            "web_search:{}",
            get("query")
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        ),
        _ => format!("{name}:{args}"),
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
        "task" => format!("start sub-agent: {}", get("description")),
        "skill" => format!("load skill {}", get("name")),
        "browser" => match get("action").as_str() {
            "open" => format!("browser: open {}", get("url")),
            "type" => format!("browser: type into {}", get("ref")),
            "find" => format!("browser: find \"{}\"", get("text")),
            "press" => format!("browser: press {}", get("key")),
            a @ ("click" | "select") => format!("browser: {a} {}", get("ref")),
            a => format!("browser: {a}"),
        },
        "mcp" => format!("list tools on {}", get("server")),
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

/// Longest tool output carried in an event (interfaces show it on demand).
const EVENT_OUTPUT_CHARS: usize = 6_000;
const DETAIL_LINES: usize = 80;

/// What a tool call will change, as diff-style lines ("- old", "+ new").
pub fn change_detail(name: &str, args: &Value) -> String {
    let get = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    let prefixed = |text: &str, mark: &str| -> Vec<String> {
        let mut lines: Vec<String> = text
            .lines()
            .take(DETAIL_LINES)
            .map(|l| format!("{mark} {l}"))
            .collect();
        let more = text.lines().count().saturating_sub(DETAIL_LINES);
        if more > 0 {
            lines.push(format!("  … {more} more lines"));
        }
        lines
    };
    match name {
        "edit_file" => {
            let mut lines = prefixed(get("old_string"), "-");
            lines.extend(prefixed(get("new_string"), "+"));
            lines.join("\n")
        }
        "write_file" => prefixed(get("content"), "+").join("\n"),
        "shell" => format!("$ {}", get("command")),
        _ => String::new(),
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

    #[tokio::test]
    async fn recall_injects_memories_into_system_prompt() {
        struct MockRecall;
        impl crate::extract::MemoryRecall for MockRecall {
            fn recall_for_prompt<'a>(
                &'a self,
                prompt: &'a str,
                _project: &'a str,
                _budget: usize,
            ) -> crate::extract::RecallFuture<'a> {
                let res = if prompt.contains("rust") {
                    vec![
                        "[user_pref] User loves Rust".into(),
                        "[project_fact] Cargo workspace".into(),
                    ]
                } else {
                    vec![]
                };
                Box::pin(async move { res })
            }
        }

        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        a.set_memory_recall(Arc::new(MockRecall));

        let mut sink = |_e: AgentEvent| {};
        // Turn 1: matches rust
        let _ = a.run_turn("tell me about rust", &mut sink).await;
        let sys_content = a.session.messages[0].content.as_deref().unwrap();
        assert!(sys_content.contains("<relevant_memories>"));
        assert!(sys_content.contains("User loves Rust"));
        assert!(sys_content.contains("Cargo workspace"));

        // Turn 2: does not match rust -> resets to base prompt
        let _ = a.run_turn("hello world", &mut sink).await;
        let sys_content2 = a.session.messages[0].content.as_deref().unwrap();
        assert!(!sys_content2.contains("<relevant_memories>"));
        assert_eq!(sys_content2, "sys");
    }

    #[test]
    fn session_resume_and_workspace_switch() {
        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        assert_eq!(a.tools.root(), d.path());

        // 1. Switch workspace
        let other_dir = tempfile::tempdir().unwrap();
        a.set_workspace(other_dir.path().to_path_buf());
        assert_eq!(a.tools.root(), other_dir.path());
        assert_eq!(a.session.workspace, other_dir.path());

        // 2. New session with custom workspace
        let third_dir = tempfile::tempdir().unwrap();
        a.new_session(third_dir.path().to_path_buf());
        assert_eq!(a.tools.root(), third_dir.path());
        assert_eq!(a.session.workspace, third_dir.path());

        // 3. Resume old session updates tools root to old workspace
        let mut old = Session::new(d.path().to_path_buf());
        old.messages.push(Message::user("prior query"));
        a.resume(old);
        assert_eq!(a.tools.root(), d.path());
        assert_eq!(a.session.workspace, d.path());
        assert_eq!(a.session.messages.len(), 2); // system + user
    }

    #[tokio::test]
    async fn test_spoken_reply_injects_instruction() {
        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        let mut sink = |_e: AgentEvent| {};

        // Normal run: does not contain spoken instruction
        let _ = a.run_turn("what time is it", &mut sink).await;
        assert_eq!(a.session.messages[0].content.as_deref().unwrap(), "sys");

        // Spoken reply mode enabled
        a.spoken_reply = true;
        let _ = a.run_turn("what is the weather", &mut sink).await;
        let spoken_prompt = a.session.messages[0].content.as_deref().unwrap();
        assert!(spoken_prompt.contains("Voice Mode & Spoken Reply Style"));
        assert!(spoken_prompt.contains("Do not read long code blocks"));
    }

    #[tokio::test]
    async fn test_run_turn_with_images_classifies_vision() {
        let d = tempfile::tempdir().unwrap();
        let mut a = agent(d.path());
        let mut sink = |_e: AgentEvent| {};

        let img = oblivion_providers::ImageAttachment::new(
            "image/png",
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=",
        );
        let _ = a
            .run_turn_with_images("describe this screenshot", vec![img], &mut sink)
            .await;

        let last_msg = a.session.messages.last().expect("expected user message");
        assert_eq!(last_msg.role, Role::User);
        assert_eq!(
            last_msg.content.as_deref(),
            Some("describe this screenshot")
        );
        assert_eq!(last_msg.images.len(), 1);
        assert_eq!(last_msg.images[0].mime_type, "image/png");
    }
}
