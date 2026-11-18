//! The agent behind the full-screen and web interfaces.
//!
//! One worker owns the [`Agent`] and handles one input at a time (a request or a
//! slash command). Everything it does is reported as a [`UiEvent`] on a channel;
//! approvals are asked through the same channel and answered with
//! [`Approvals::answer`]. Interfaces only send text in and read events out.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oblivion_core::{Agent, AgentEvent, Approver, Decision};
use oblivion_mcp::McpHub;
use oblivion_skills::SkillSet;
use oblivion_tools::TodoStatus;
use serde::Serialize;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;

use crate::commands::{approval_name, handle_command, Command};

pub type EventTx = UnboundedSender<UiEvent>;

/// How long an unanswered approval waits before counting as "no".
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    /// What the user sent (shown in the transcript).
    User {
        text: String,
    },
    Text {
        text: String,
    },
    TextEnd,
    ToolStart {
        name: String,
        summary: String,
        detail: String,
    },
    ToolEnd {
        name: String,
        preview: String,
        is_error: bool,
        output: String,
    },
    Declined {
        name: String,
    },
    Blocked {
        name: String,
        rule: String,
    },
    Fallback {
        from: String,
        reason: String,
    },
    Model {
        label: String,
    },
    Notice {
        text: String,
    },
    Waiting {
        secs: u64,
    },
    /// Progress of sub-agent number `index`.
    Sub {
        index: usize,
        label: String,
        event: Box<UiEvent>,
    },
    TurnEnd {
        steps: usize,
        tool_calls: usize,
        prompt_tokens: u64,
        completion_tokens: u64,
        /// "done", "cancelled" or "error"
        outcome: String,
        error: Option<String>,
    },
    /// The agent wants to do something that needs the user's OK.
    Approval {
        id: u64,
        tool: String,
        summary: String,
    },
    ApprovalDone {
        id: u64,
        answer: String,
    },
    /// Output of a slash command.
    Info {
        text: String,
    },
    Todos {
        items: Vec<TodoView>,
    },
    SessionResumed {
        id: String,
        workspace: String,
        messages: Vec<SessionMessageView>,
    },
    Busy {
        busy: bool,
    },
    State(StateView),
    /// `/exit` was typed.
    Quit,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SessionMessageView {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TodoView {
    pub text: String,
    /// "pending", "in_progress" or "completed"
    pub status: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelView {
    pub label: String,
    pub cooldown_secs: Option<u64>,
    pub used_today: Option<u64>,
    pub per_day: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StateView {
    pub version: String,
    pub root: String,
    pub session: String,
    pub approval: String,
    pub models: Vec<ModelView>,
    pub todos: Vec<TodoView>,
    pub skills: Vec<String>,
    pub browser: bool,
    pub instructions_file: Option<String>,
}

impl From<AgentEvent> for UiEvent {
    fn from(e: AgentEvent) -> Self {
        match e {
            AgentEvent::Text(text) => UiEvent::Text { text },
            AgentEvent::TextEnd => UiEvent::TextEnd,
            AgentEvent::ToolStart {
                name,
                summary,
                detail,
            } => UiEvent::ToolStart {
                name,
                summary,
                detail,
            },
            AgentEvent::ToolEnd {
                name,
                preview,
                is_error,
                output,
            } => UiEvent::ToolEnd {
                name,
                preview,
                is_error,
                output,
            },
            AgentEvent::Declined { name } => UiEvent::Declined { name },
            AgentEvent::Blocked { name, rule } => UiEvent::Blocked { name, rule },
            AgentEvent::Fallback { from, reason } => UiEvent::Fallback {
                from,
                reason: short_reason(&reason),
            },
            AgentEvent::Model(label) => UiEvent::Model { label },
            AgentEvent::Notice(text) => UiEvent::Notice { text },
            AgentEvent::Waiting(secs) => UiEvent::Waiting { secs },
            AgentEvent::Sub {
                index,
                label,
                event,
            } => UiEvent::Sub {
                index,
                label,
                event: Box::new(UiEvent::from(*event)),
            },
        }
    }
}

/// Provider errors carry the whole JSON body; the first part says enough.
pub fn short_reason(reason: &str) -> String {
    let r = reason.split(": {").next().unwrap_or(reason);
    if r.chars().count() > 160 {
        format!("{}…", r.chars().take(160).collect::<String>())
    } else {
        r.to_string()
    }
}

fn todo_views(items: &[oblivion_tools::TodoItem]) -> Vec<TodoView> {
    items
        .iter()
        .map(|t| TodoView {
            text: t.content.clone(),
            status: match t.status {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            }
            .into(),
        })
        .collect()
}

pub fn state_view(agent: &Agent, skills: &SkillSet, browser: bool) -> StateView {
    StateView {
        version: env!("CARGO_PKG_VERSION").into(),
        root: agent.tools.root().display().to_string(),
        session: agent.session.id.clone(),
        approval: approval_name(agent.approval).into(),
        models: agent
            .router
            .status()
            .into_iter()
            .map(|s| ModelView {
                label: s.label,
                cooldown_secs: s.cooldown.map(|d| d.as_secs().max(1)),
                used_today: s.used_today,
                per_day: s.requests_per_day,
            })
            .collect(),
        todos: todo_views(&agent.tools.todos().items()),
        skills: skills.all().map(|s| s.name.clone()).collect(),
        browser,
        instructions_file: oblivion_core::prompt::project_instructions(agent.tools.root())
            .map(|(name, _)| name),
    }
}

/// Approval requests waiting for an answer from the interface.
pub struct Approvals {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, std::sync::mpsc::Sender<Decision>>>,
    tx: EventTx,
}

impl Approvals {
    pub fn new(tx: EventTx) -> Arc<Self> {
        Arc::new(Self {
            next: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            tx,
        })
    }

    /// Answer one request; false if it was already answered or unknown.
    pub fn answer(&self, id: u64, decision: Decision) -> bool {
        match self.pending.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(decision).is_ok(),
            None => false,
        }
    }

    /// Say no to everything waiting (used when a request is cancelled).
    pub fn decline_all(&self) {
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Decision::No);
        }
    }

    pub fn approver(self: &Arc<Self>) -> Box<dyn Approver> {
        Box::new(ChannelApprover(self.clone()))
    }
}

struct ChannelApprover(Arc<Approvals>);

impl Approver for ChannelApprover {
    fn approve(&self, tool: &str, summary: &str) -> Decision {
        let a = &self.0;
        let id = a.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        a.pending.lock().unwrap().insert(id, tx);
        let _ = a.tx.send(UiEvent::Approval {
            id,
            tool: tool.into(),
            summary: summary.into(),
        });
        // The agent calls this inside `block_in_place`, so waiting here does not
        // stall the rest of the program.
        let decision = rx.recv_timeout(APPROVAL_TIMEOUT).unwrap_or(Decision::No);
        a.pending.lock().unwrap().remove(&id);
        let answer = match decision {
            Decision::Yes => "yes",
            Decision::No => "no",
            Decision::Always => "always",
        };
        let _ = a.tx.send(UiEvent::ApprovalDone {
            id,
            answer: answer.into(),
        });
        decision
    }
}

/// What an interface holds to drive the worker.
#[derive(Clone)]
pub struct Controls {
    pub input: UnboundedSender<String>,
    pub approvals: Arc<Approvals>,
    cancel: Arc<Notify>,
    busy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Controls {
    /// Stop the running request (also says no to any open approval).
    pub fn cancel(&self) {
        // Signal "stop" before releasing a waiting approval, so the agent cannot
        // move on to its next step in between.
        self.stop.store(true, Ordering::SeqCst);
        self.cancel.notify_waiters();
        self.approvals.decline_all();
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn send(&self, text: impl Into<String>) {
        let _ = self.input.send(text.into());
    }
}

pub struct Worker {
    pub agent: Agent,
    pub skills: Arc<SkillSet>,
    pub hub: Arc<McpHub>,
    pub browser: bool,
    input: UnboundedReceiver<String>,
    tx: EventTx,
    cancel: Arc<Notify>,
    busy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    approvals: Arc<Approvals>,
    traces_dir: Option<PathBuf>,
    tracer_slot: Option<Arc<Mutex<Option<Arc<crate::trace::Tracer>>>>>,
    store: Option<Arc<oblivion_store::Store>>,
}

/// Build the channels first: the agent needs `approvals.approver()` when it is created.
pub fn channels() -> (EventTx, UnboundedReceiver<UiEvent>) {
    tokio::sync::mpsc::unbounded_channel()
}

impl Worker {
    pub fn new(
        agent: Agent,
        skills: Arc<SkillSet>,
        hub: Arc<McpHub>,
        browser: bool,
        tx: EventTx,
        approvals: Arc<Approvals>,
    ) -> (Self, Controls) {
        let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel();
        let cancel = Arc::new(Notify::new());
        let busy = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let controls = Controls {
            input: in_tx,
            approvals: approvals.clone(),
            cancel: cancel.clone(),
            busy: busy.clone(),
            stop: stop.clone(),
        };
        (
            Self {
                agent,
                skills,
                hub,
                browser,
                input: in_rx,
                tx,
                cancel,
                busy,
                stop,
                approvals,
                traces_dir: None,
                tracer_slot: None,
                store: None,
            },
            controls,
        )
    }

    pub fn with_store(mut self, store: Arc<oblivion_store::Store>) -> Self {
        self.store = Some(store);
        self
    }

    pub fn with_traces(
        mut self,
        dir: PathBuf,
        slot: Arc<Mutex<Option<Arc<crate::trace::Tracer>>>>,
    ) -> Self {
        self.traces_dir = Some(dir);
        self.tracer_slot = Some(slot);
        self
    }

    fn send_state(&self) {
        let _ = self.tx.send(UiEvent::State(state_view(
            &self.agent,
            &self.skills,
            self.browser,
        )));
    }

    /// Handle inputs until every [`Controls`] is dropped or `/exit` is typed.
    pub async fn run(mut self) {
        self.send_state();
        while let Some(text) = self.input.recv().await {
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let _ = self.tx.send(UiEvent::User { text: text.clone() });
            let task = match text.strip_prefix('/') {
                Some(cmd) => {
                    let tx = self.tx.clone();
                    let mut sink = move |e: AgentEvent| {
                        let _ = tx.send(e.into());
                    };
                    let result = handle_command(
                        &mut self.agent,
                        &self.skills,
                        &self.hub,
                        self.store.as_deref(),
                        cmd,
                        &mut sink,
                    )
                    .await;
                    match result {
                        Ok(Command::Quit) => {
                            let _ = self.tx.send(UiEvent::Quit);
                            break;
                        }
                        Ok(Command::Done(info)) => {
                            let _ = self.tx.send(UiEvent::Info { text: info });
                            if text.starts_with("/resume ") {
                                let messages = self
                                    .agent
                                    .session
                                    .messages
                                    .iter()
                                    .filter_map(|m| {
                                        let role = match m.role {
                                            oblivion_providers::Role::User => "user",
                                            oblivion_providers::Role::Assistant => "assistant",
                                            _ => return None,
                                        };
                                        m.content.as_ref().map(|c| SessionMessageView {
                                            role: role.into(),
                                            content: c.clone(),
                                        })
                                    })
                                    .collect();
                                let _ = self.tx.send(UiEvent::SessionResumed {
                                    id: self.agent.session.id.clone(),
                                    workspace: self.agent.tools.root().display().to_string(),
                                    messages,
                                });
                            }
                            self.send_state();
                            continue;
                        }
                        Ok(Command::Run(task)) => task,
                        Err(e) => {
                            let _ = self.tx.send(UiEvent::Info {
                                text: format!("✗ {e:#}"),
                            });
                            continue;
                        }
                    }
                }
                None => text,
            };
            self.run_turn(&task).await;
        }
    }

    async fn run_turn(&mut self, task: &str) {
        self.stop.store(false, Ordering::SeqCst);
        self.busy.store(true, Ordering::SeqCst);
        let _ = self.tx.send(UiEvent::Busy { busy: true });
        let tx = self.tx.clone();
        let todos = self.agent.tools.todos_shared();

        let tracer = self.traces_dir.as_ref().and_then(|dir| {
            crate::trace::Tracer::start(dir, task, self.agent.tools.root()).map(Arc::new)
        });
        if let (Some(slot), Some(t)) = (&self.tracer_slot, &tracer) {
            *slot.lock().unwrap() = Some(t.clone());
        }
        let tracer_ev = tracer.clone();

        let mut on_event = move |e: AgentEvent| {
            if let Some(t) = &tracer_ev {
                match &e {
                    AgentEvent::Model(m) => t.record_model(m),
                    AgentEvent::Fallback { from, reason } => t.record_fallback(from, reason),
                    AgentEvent::ToolStart {
                        name,
                        summary,
                        detail,
                    } => {
                        t.record_tool_start(name, summary, detail);
                    }
                    AgentEvent::ToolEnd {
                        name,
                        preview,
                        is_error,
                        ..
                    } => {
                        t.record_tool_end(name, preview, *is_error);
                    }
                    AgentEvent::Declined { name } => t.record_declined(name),
                    AgentEvent::Notice(msg) => t.record(crate::trace::TraceEntry::Notice {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        text: msg.clone(),
                    }),
                    _ => {}
                }
            }
            let todo_changed = matches!(&e, AgentEvent::ToolEnd { name, .. } if name == "todo");
            let _ = tx.send(e.into());
            if todo_changed {
                let _ = tx.send(UiEvent::Todos {
                    items: todo_views(&todos.items()),
                });
            }
        };
        // Register for "stop" before starting: a stop pressed while the agent waits
        // for an approval must not be missed.
        let cancel = self.cancel.clone();
        let stopped = cancel.notified();
        tokio::pin!(stopped);
        stopped.as_mut().enable();
        let result = tokio::select! {
            biased;
            _ = &mut stopped => None,
            r = self.agent.run_turn(task, &mut on_event) => Some(r),
        };
        // The request can also finish in the same moment Stop is pressed (a declined
        // approval followed by an instant error); Stop still wins.
        let result = if self.stop.swap(false, Ordering::SeqCst) {
            None
        } else {
            result
        };
        let end = match result {
            None => {
                if let Some(t) = &tracer {
                    t.finish(0, 0, 0, 0, "cancelled", None);
                }
                self.approvals.decline_all();
                self.agent.repair();
                UiEvent::TurnEnd {
                    steps: 0,
                    tool_calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    outcome: "cancelled".into(),
                    error: None,
                }
            }
            Some(Err(e)) => {
                let err_msg = format!("{e:#}");
                if let Some(t) = &tracer {
                    t.finish(0, 0, 0, 0, "error", Some(&err_msg));
                }
                UiEvent::TurnEnd {
                    steps: 0,
                    tool_calls: 0,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    outcome: "error".into(),
                    error: Some(err_msg),
                }
            }
            Some(Ok(s)) => {
                if let Some(t) = &tracer {
                    t.finish(
                        s.steps,
                        s.tool_calls,
                        s.usage.prompt_tokens,
                        s.usage.completion_tokens,
                        "done",
                        None,
                    );
                }
                UiEvent::TurnEnd {
                    steps: s.steps,
                    tool_calls: s.tool_calls,
                    prompt_tokens: s.usage.prompt_tokens,
                    completion_tokens: s.usage.completion_tokens,
                    outcome: "done".into(),
                    error: None,
                }
            }
        };
        if let Some(slot) = &self.tracer_slot {
            *slot.lock().unwrap() = None;
        }
        self.busy.store(false, Ordering::SeqCst);
        let _ = self.tx.send(end);
        let _ = self.tx.send(UiEvent::Busy { busy: false });
        self.send_state();
    }
}
