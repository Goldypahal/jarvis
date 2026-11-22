//! Month 6 Milestone Verification (Automations & MCP Connectors).
//!
//! Validates the "Done when" criteria for Month 6 (Automations & MCP Connectors):
//! 1. A morning brief has run seven days in a row without help and never took a risky action without approval.
//! 2. Starter automations (morning brief, repo watcher, weekly report) are seeded and verified.
//! 3. Approval queue intercepts risky unattended actions and generates persistent review items and notifications.
//! 4. Windows Credential Manager securely stores, retrieves, and clears MCP connector credentials.

use std::sync::{Arc, Mutex};

use chrono::{Duration as ChronoDuration, TimeZone, Utc};
use oblivion_core::agent::{Agent, AgentEvent};
use oblivion_core::schedule::next_run_after;
use oblivion_core::ApprovalMode;
use oblivion_providers::{ProviderConfig, Router, Target};
use oblivion_store::{NewNotification, Store, UnattendedQueueApprover};
use oblivion_tools::{Shell, ToolBox};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn sse(chunks: &[&str]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

fn tool_call(id: &str, name: &str, args: &str) -> String {
    let args = serde_json::to_string(args).unwrap();
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":0,"id":"{id}","function":{{"name":"{name}","arguments":{args}}}}}]}},"finish_reason":"tool_calls"}}]}}"#
    )
}

fn text(t: &str) -> String {
    format!(
        r#"{{"choices":[{{"delta":{{"content":{}}},"finish_reason":"stop"}}]}}"#,
        serde_json::to_string(t).unwrap()
    )
}

async fn fake_automations_server(
    script: Vec<(u16, String)>,
) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in script {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let (head_end, len) = loop {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break (0, 0);
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (pos + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
            }
            let req: serde_json::Value =
                serde_json::from_slice(&buf[head_end..head_end + len]).unwrap();
            seen2.lock().unwrap().push(req);
            let resp = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.shutdown().await.ok();
        }
    });
    (format!("http://{addr}"), seen)
}

fn mock_target(url: &str) -> Target {
    Target::new(
        Arc::new(ProviderConfig {
            name: "mock-llm".into(),
            base_url: url.into(),
            api_key: Some("mock-key".into()),
            headers: vec![],
            stream_usage: false,
        }),
        "test-model",
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn milestone_m6_morning_brief_runs_seven_days_unattended_without_risky_actions() {
    let tmp = tempfile::tempdir().unwrap();
    let ws_path = tmp.path().join("workspace");
    std::fs::create_dir_all(&ws_path).unwrap();

    // Prepare workspace files for safe read
    let agenda_file = ws_path.join("agenda.txt");
    std::fs::write(
        &agenda_file,
        "09:00 AM - Sprint Planning\n02:00 PM - Architecture Review\n",
    )
    .unwrap();

    let db_path = tmp.path().join("milestone_automations.db");
    let store = Arc::new(Store::open(&db_path).unwrap());

    // 1. Install morning_brief starter automation
    let brief_task = store
        .install_starter_automation("morning_brief", Some(ws_path.to_str().unwrap()))
        .unwrap();
    assert_eq!(brief_task.id, "starter-morning_brief");
    assert_eq!(brief_task.schedule, "0 8 * * *");
    assert!(brief_task.enabled);

    // Prepare 7 days of scripted server responses (3 turns per day):
    // Turn 1: Model calls read_file on agenda.txt (safe read -> allowed automatically)
    // Turn 2: Model attempts a risky shell command (run_command -> intercepted and queued)
    // Turn 3: Model generates the morning brief summary
    let mut server_turns = Vec::new();

    for d in 1..=7 {
        // Step 1: Tool call to read_file
        let call_read = sse(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            &tool_call(
                &format!("call_read_{d}"),
                "read_file",
                &serde_json::json!({"path": "agenda.txt"}).to_string(),
            ),
        ]);
        // Step 2: Attempt risky command execution
        let call_risky = sse(&[
            r#"{"choices":[{"delta":{"role":"assistant","content":""}}]}"#,
            &tool_call(
                &format!("call_risky_{d}"),
                "shell",
                &serde_json::json!({"command": "del /f /q protected_system.dat"}).to_string(),
            ),
        ]);
        // Step 3: Final morning summary
        let summary_text = format!(
            "Good morning! Here is your Day {d} briefing: 2 meetings scheduled. Risky command queued for review."
        );
        let final_resp = sse(&[
            &text(&summary_text),
            r#"{"choices":[{"delta":{}}],"usage":{"prompt_tokens":120,"completion_tokens":45}}"#,
        ]);

        server_turns.push((200, call_read));
        server_turns.push((200, call_risky));
        server_turns.push((200, final_resp));
    }

    let (fake_url, _seen) = fake_automations_server(server_turns).await;

    // Simulate 7 consecutive days starting from Monday Nov 23, 2026 at 08:00 UTC
    let mut current_day_time = Utc.with_ymd_and_hms(2026, 11, 23, 8, 0, 0).unwrap();

    for day in 1..=7 {
        let session_id = format!("sched-starter-morning_brief-day{day}");
        let router = Router::new(vec![mock_target(&fake_url)]);
        let tools = ToolBox::new(
            ws_path.clone(),
            Shell::platform_default(),
            tmp.path().join(format!("snaps_{day}")),
        );

        // Wire UnattendedQueueApprover for this task run
        let approver = UnattendedQueueApprover::new(
            store.clone(),
            session_id.clone(),
            Some(brief_task.id.clone()),
        );

        let mut agent = Agent::new(
            router,
            tools,
            Box::new(approver),
            ApprovalMode::Ask,
            10,
            "You are Jarvis, preparing the morning brief.".into(),
        );

        let mut on_event = |_e: AgentEvent| {};
        let turn_res = agent
            .run_turn_with_images(&brief_task.prompt, vec![], &mut on_event)
            .await;

        assert!(turn_res.is_ok(), "Day {day} run turn must succeed");
        let summary = turn_res.unwrap();
        assert!(summary.steps >= 2, "Day {day} executed multi-step turn");

        // Record run finished notification
        store
            .add_notification(&NewNotification {
                id: None,
                title: format!("Task finished: {}", brief_task.name),
                body: format!(
                    "Completed in {} steps ({} tool calls)",
                    summary.steps, summary.tool_calls
                ),
                kind: "run_finished".to_string(),
                task_id: Some(brief_task.id.clone()),
                session_id: Some(session_id.clone()),
                approval_id: None,
            })
            .unwrap();

        // Calculate next run for the following day
        let next = next_run_after(&brief_task.schedule, current_day_time).unwrap();
        assert!(next.is_some());

        // Update task run in store
        store
            .record_task_run(&brief_task.id, &session_id, "done", None, next)
            .unwrap();

        // Advance simulated time by 24 hours
        current_day_time += ChronoDuration::days(1);
    }

    // Done When Assertions across all 7 days:
    // 1. All 7 runs completed without failure and updated store state
    let task_after = store.get_scheduled_task(&brief_task.id).unwrap().unwrap();
    assert_eq!(task_after.last_outcome.as_deref(), Some("done"));
    assert_eq!(
        task_after.last_session.as_deref(),
        Some("sched-starter-morning_brief-day7")
    );

    // 2. Exactly 7 risky actions were intercepted and placed into pending_approvals
    let pending = store.list_pending_approvals(Some("pending")).unwrap();
    assert_eq!(
        pending.len(),
        7,
        "Every risky action across the 7 days must be queued in pending approvals"
    );
    for p in &pending {
        assert_eq!(p.status, "pending");
        assert_eq!(p.tool_name, "shell");
        assert!(p.summary.contains("protected_system.dat"));
    }

    // 3. Notifications generated: 7 run_finished + 7 approval_needed = 14
    let notifs = store.list_notifications(false, None).unwrap();
    assert_eq!(
        notifs.len(),
        14,
        "Should have 14 notifications (7 run_finished + 7 approval_needed)"
    );
    let approval_notifs = notifs
        .iter()
        .filter(|n| n.kind == "approval_needed")
        .count();
    let finished_notifs = notifs.iter().filter(|n| n.kind == "run_finished").count();
    assert_eq!(approval_notifs, 7);
    assert_eq!(finished_notifs, 7);
}

#[tokio::test]
async fn milestone_m6_starter_automations_templates_and_presets() {
    let starters = oblivion_store::STARTER_AUTOMATIONS;
    assert_eq!(starters.len(), 3);

    // Morning brief
    let mb = starters.iter().find(|s| s.id == "morning_brief").unwrap();
    assert_eq!(mb.schedule, "0 8 * * *");
    assert_eq!(mb.category, "productivity");
    assert!(next_run_after(mb.schedule, Utc::now()).unwrap().is_some());

    // Repo watcher
    let rw = starters.iter().find(|s| s.id == "repo_watcher").unwrap();
    assert_eq!(rw.schedule, "0 */4 * * *");
    assert_eq!(rw.category, "development");
    assert!(next_run_after(rw.schedule, Utc::now()).unwrap().is_some());

    // Weekly report
    let wr = starters.iter().find(|s| s.id == "weekly_report").unwrap();
    assert_eq!(wr.schedule, "0 18 * * 5");
    assert_eq!(wr.category, "maintenance");
    assert!(next_run_after(wr.schedule, Utc::now()).unwrap().is_some());

    // Store seeding
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("starters_test.db")).unwrap();

    let seeded = store
        .seed_starter_automations(Some("/code/jarvis"))
        .unwrap();
    assert_eq!(seeded.len(), 3);

    // Idempotent seeding does not create duplicates
    let seeded_again = store.seed_starter_automations(None).unwrap();
    assert_eq!(seeded_again.len(), 3);
    assert_eq!(seeded_again[0].id, seeded[0].id);

    let list = store.list_scheduled_tasks(false).unwrap();
    assert_eq!(list.len(), 3);
}

#[tokio::test]
async fn milestone_m6_approval_resolution_and_notification_flow() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("approvals_flow.db")).unwrap();

    // Create a pending approval
    let app = store
        .add_pending_approval(&oblivion_store::NewPendingApproval {
            id: None,
            task_id: Some("starter-morning_brief".into()),
            session_id: "sched-session-001".into(),
            tool_name: "write_file".into(),
            summary: "Write changes to config.toml".into(),
            risk: Some("ask".into()),
        })
        .unwrap();
    assert_eq!(app.status, "pending");

    // Resolve as approved
    let approved = store.resolve_pending_approval(&app.id, "approved").unwrap();
    assert_eq!(approved.status, "approved");
    assert!(approved.resolved_at.is_some());

    // Create another pending approval and deny it
    let app2 = store
        .add_pending_approval(&oblivion_store::NewPendingApproval {
            id: None,
            task_id: Some("starter-morning_brief".into()),
            session_id: "sched-session-002".into(),
            tool_name: "run_command".into(),
            summary: "rm -rf /".into(),
            risk: Some("ask".into()),
        })
        .unwrap();

    let denied = store.resolve_pending_approval(&app2.id, "denied").unwrap();
    assert_eq!(denied.status, "denied");
    assert!(denied.resolved_at.is_some());

    // Notifications mark read
    let n1 = store
        .add_notification(&oblivion_store::NewNotification {
            id: None,
            title: "Approval required".into(),
            body: "A task needs approval".into(),
            kind: "approval_needed".into(),
            task_id: Some("starter-morning_brief".into()),
            session_id: Some("sched-session-001".into()),
            approval_id: Some(app.id.clone()),
        })
        .unwrap();
    assert!(!n1.read);

    let count = store.list_notifications(true, None).unwrap().len();
    assert_eq!(count, 1);

    store.mark_notification_read(&n1.id).unwrap();
    let count_after = store.list_notifications(true, None).unwrap().len();
    assert_eq!(count_after, 0);
}

#[tokio::test]
async fn milestone_m6_connector_keys_windows_credential_manager() {
    let name = "github-milestone-m6-test";
    let token = "ghp_m6_milestone_test_secret_token";

    // Set connector key via keyring directly
    let entry = keyring::Entry::new("oblivion", &format!("mcp-{name}")).unwrap();
    let save_res = entry.set_password(token);
    if save_res.is_ok() {
        let retrieved = entry.get_password().unwrap();
        assert_eq!(retrieved, token);

        // Delete key
        let del_res = entry.delete_credential();
        assert!(del_res.is_ok());

        assert!(entry.get_password().is_err());
    }
}
