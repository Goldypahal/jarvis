//! Scheduled task runner: executes tasks unattended as fresh sessions with trace logging.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use oblivion_core::agent::{Agent, AgentEvent};
use oblivion_core::schedule::{next_run_after, UnattendedApprover};
use oblivion_core::session::Session;
use oblivion_store::{ScheduledTask, Store};

use crate::trace::{TraceEntry, Tracer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledRunResult {
    pub task_id: String,
    pub session_id: String,
    pub outcome: String,
    pub error: Option<String>,
    pub steps: usize,
    pub tool_calls: usize,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub trace_id: Option<String>,
}

/// Runs a scheduled task unattended:
/// 1. Creates a fresh session for the task run.
/// 2. Initializes a per-run trace file under `traces_dir`.
/// 3. Executes the agent turn with `UnattendedApprover`.
/// 4. Records the trace finish and updates the task status in SQLite Store.
pub async fn execute_scheduled_task(
    store: &Arc<Store>,
    task: &ScheduledTask,
    agent: &mut Agent,
    traces_dir: &Path,
) -> Result<ScheduledRunResult> {
    let now = Utc::now();
    let session_id = format!(
        "sched-{}-{:x}",
        task.id,
        now.timestamp_nanos_opt().unwrap_or(0)
    );
    let workspace = task
        .workspace
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| agent.tools.root().to_path_buf());

    // 1. Fresh session
    let mut session = Session::new(workspace.clone());
    session.id = session_id.clone();
    agent.session = session;

    // Set unattended approver so low-risk runs and risky actions do not execute blindly
    agent.set_approver(Box::new(UnattendedApprover));

    // 2. Start trace
    let tracer = Tracer::start(traces_dir, &task.prompt, &workspace).map(Arc::new);
    let trace_id = tracer.as_ref().map(|t| t.run_id.clone());
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
                AgentEvent::Notice(msg) => t.record(TraceEntry::Notice {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    text: msg.clone(),
                }),
                _ => {}
            }
        }
    };

    // 3. Run turn
    let turn_res = agent
        .run_turn_with_images(&task.prompt, vec![], &mut on_event)
        .await;

    // 4. Trace completion and outcome
    let (outcome, error, steps, tool_calls, prompt_tokens, completion_tokens) = match turn_res {
        Ok(summary) => {
            if let Some(t) = &tracer {
                t.finish(
                    summary.steps,
                    summary.tool_calls,
                    summary.usage.prompt_tokens,
                    summary.usage.completion_tokens,
                    "done",
                    None,
                );
            }
            (
                "done".to_string(),
                None,
                summary.steps,
                summary.tool_calls,
                summary.usage.prompt_tokens,
                summary.usage.completion_tokens,
            )
        }
        Err(e) => {
            let err_msg = format!("{e:#}");
            if let Some(t) = &tracer {
                t.finish(0, 0, 0, 0, "error", Some(&err_msg));
            }
            ("error".to_string(), Some(err_msg), 0, 0, 0, 0)
        }
    };

    // 5. Calculate next run
    let next_run = if task.is_recurring {
        next_run_after(&task.schedule, now).unwrap_or(None)
    } else {
        None
    };

    // 6. Record run in Store
    store
        .record_task_run(&task.id, &session_id, &outcome, error.as_deref(), next_run)
        .context("recording scheduled task run in SQLite store")?;

    Ok(ScheduledRunResult {
        task_id: task.id.clone(),
        session_id,
        outcome,
        error,
        steps,
        tool_calls,
        prompt_tokens,
        completion_tokens,
        trace_id,
    })
}

pub type AgentFactory = Arc<dyn Fn(&Path) -> Result<Agent> + Send + Sync>;

pub struct ScheduleRunner {
    pub store: Arc<Store>,
    pub traces_dir: PathBuf,
    pub factory: AgentFactory,
}

impl ScheduleRunner {
    pub fn new(store: Arc<Store>, traces_dir: PathBuf, factory: AgentFactory) -> Self {
        Self {
            store,
            traces_dir,
            factory,
        }
    }

    /// Run a specific scheduled task by ID immediately.
    pub async fn run_task(&self, task_id: &str) -> Result<ScheduledRunResult> {
        let task = self
            .store
            .get_scheduled_task(task_id)
            .context("fetching task from store")?
            .ok_or_else(|| anyhow::anyhow!("Scheduled task not found: {task_id}"))?;
        let workspace = task
            .workspace
            .as_deref()
            .map(Path::new)
            .unwrap_or(Path::new("."));
        let mut agent =
            (self.factory)(workspace).context("instantiating fresh agent for scheduled task")?;
        execute_scheduled_task(&self.store, &task, &mut agent, &self.traces_dir).await
    }

    /// Check for any due tasks and run them.
    pub async fn tick(&self) -> Result<Vec<ScheduledRunResult>> {
        let now = Utc::now();
        let due_tasks = self
            .store
            .due_scheduled_tasks(now)
            .context("fetching due scheduled tasks")?;
        let mut results = Vec::new();
        for task in due_tasks {
            match self.run_task(&task.id).await {
                Ok(res) => results.push(res),
                Err(e) => {
                    eprintln!("Scheduled task {} failed to run: {:#}", task.id, e);
                }
            }
        }
        Ok(results)
    }

    /// Starts a background ticker loop checking for due tasks at `interval_secs`.
    pub fn start_ticker(self: Arc<Self>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                if let Err(e) = self.tick().await {
                    eprintln!("Scheduler tick failed: {:#}", e);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{fake_server, sse, target};
    use oblivion_core::ApprovalMode;
    use oblivion_providers::Router;
    use oblivion_store::NewScheduledTask;
    use oblivion_tools::{Shell, ToolBox};

    #[tokio::test]
    async fn test_execute_scheduled_task_creates_fresh_session_and_trace() {
        let store = Arc::new(Store::in_memory().unwrap());
        let task = store
            .add_scheduled_task(
                &NewScheduledTask::new("Daily Cleanup", "Clean cache", "every 2h")
                    .with_workspace("/tmp/test-project"),
            )
            .unwrap();

        let sse_body = sse(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":"Cache is clean."}}]}"#,
            r#"{"choices":[{"delta":{}}],"usage":{"prompt_tokens":20,"completion_tokens":10}}"#,
        ]);
        let (url, _seen) = fake_server(vec![(200, sse_body)]).await;
        let t = target("mock", &url);
        let router = Router::new(vec![t]);

        let tmp = tempfile::tempdir().unwrap();
        let traces_dir = tmp.path().join("traces");
        std::fs::create_dir_all(&traces_dir).unwrap();

        let tools = ToolBox::new(
            tmp.path().to_path_buf(),
            Shell::platform_default(),
            tmp.path().join("snaps"),
        );
        let mut agent = Agent::new(
            router,
            tools,
            Box::new(UnattendedApprover),
            ApprovalMode::Ask,
            5,
            "System prompt".into(),
        );

        let res = execute_scheduled_task(&store, &task, &mut agent, &traces_dir)
            .await
            .unwrap();

        assert_eq!(res.outcome, "done");
        assert!(res.session_id.starts_with("sched-"));
        assert!(res.trace_id.is_some());

        // Verify trace file exists on disk
        let trace_id = res.trace_id.unwrap();
        let trace_file = traces_dir.join(format!("{trace_id}.jsonl"));
        assert!(trace_file.exists());
        let trace_content = std::fs::read_to_string(&trace_file).unwrap();
        assert!(trace_content.contains("Clean cache"));
        assert!(trace_content.contains(r#""outcome":"done""#));

        // Verify store updated with last_run, last_outcome, and next_run
        let updated = store.get_scheduled_task(&task.id).unwrap().unwrap();
        assert_eq!(updated.last_outcome.as_deref(), Some("done"));
        assert_eq!(
            updated.last_session.as_deref(),
            Some(res.session_id.as_str())
        );
        assert!(updated.last_run.is_some());
        assert!(updated.next_run.is_some());
    }

    #[tokio::test]
    async fn test_schedule_runner_tick_and_manual_run() {
        let store = Arc::new(Store::in_memory().unwrap());
        let past = Utc::now() - chrono::Duration::minutes(5);
        let future = Utc::now() + chrono::Duration::hours(5);

        // Due task
        let due_task = store
            .add_scheduled_task(&NewScheduledTask::new("Due task", "Say hello", "every 1h"))
            .unwrap();
        store
            .update_scheduled_task(
                &due_task.id,
                &oblivion_store::UpdateScheduledTask {
                    next_run: Some(Some(past)),
                    ..Default::default()
                },
            )
            .unwrap();

        // Future task
        let future_task = store
            .add_scheduled_task(&NewScheduledTask::new(
                "Future task",
                "Say later",
                "every 10m",
            ))
            .unwrap();
        store
            .update_scheduled_task(
                &future_task.id,
                &oblivion_store::UpdateScheduledTask {
                    next_run: Some(Some(future)),
                    ..Default::default()
                },
            )
            .unwrap();

        let sse_body = sse(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":"Done work."}}]}"#,
            r#"{"choices":[{"delta":{}}],"usage":{"prompt_tokens":10,"completion_tokens":5}}"#,
        ]);
        let (url, _seen) = fake_server(vec![(200, sse_body.clone()), (200, sse_body)]).await;
        let url_clone = url.clone();

        let tmp = tempfile::tempdir().unwrap();
        let traces_dir = tmp.path().join("traces");
        std::fs::create_dir_all(&traces_dir).unwrap();
        let tmp_root = tmp.path().to_path_buf();

        let factory: AgentFactory = Arc::new(move |_ws| {
            let t = target("mock", &url_clone);
            let router = Router::new(vec![t]);
            let tools = ToolBox::new(
                tmp_root.clone(),
                Shell::platform_default(),
                tmp_root.join("snaps"),
            );
            Ok(Agent::new(
                router,
                tools,
                Box::new(UnattendedApprover),
                ApprovalMode::Ask,
                5,
                "System prompt".into(),
            ))
        });

        let runner = ScheduleRunner::new(store.clone(), traces_dir, factory);

        // 1. Tick should run only the due task
        let tick_res = runner.tick().await.unwrap();
        assert_eq!(tick_res.len(), 1);
        assert_eq!(tick_res[0].task_id, due_task.id);
        assert_eq!(tick_res[0].outcome, "done");

        // After tick, due_task is no longer due because next_run is updated into the future
        let second_tick = runner.tick().await.unwrap();
        assert_eq!(second_tick.len(), 0);

        // 2. Manual run of the future task works immediately
        let manual_res = runner.run_task(&future_task.id).await.unwrap();
        assert_eq!(manual_res.task_id, future_task.id);
        assert_eq!(manual_res.outcome, "done");
    }
}
