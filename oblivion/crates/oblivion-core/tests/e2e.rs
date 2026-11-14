//! End-to-end: the full agent loop against a fake OpenAI-compatible server.
//! Provider "busy" always answers 429, so the router must fall back to "good",
//! which asks for a file write, then an edit, then gives a final answer.

use std::sync::{Arc, Mutex};

use oblivion_core::compact::{SUMMARY_MARKER, SUMMARY_PROMPT};
use oblivion_core::{Agent, AgentEvent, ApprovalMode, Approver, Decision, Rules};
use oblivion_providers::Message;
use oblivion_providers::{ProviderConfig, Role, Router, Target};
use oblivion_tools::{Shell, ToolBox};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct AlwaysYes;
impl Approver for AlwaysYes {
    fn approve(&self, _: &str, _: &str) -> Decision {
        Decision::Yes
    }
}

fn sse(chunks: &[&str]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

/// Serves scripted responses in order; records each request body.
async fn fake_server(script: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in script {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            // Read headers, then the body by Content-Length.
            let (head_end, len) = loop {
                let n = sock.read(&mut tmp).await.unwrap();
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
            let reason = if status == 200 {
                "OK"
            } else {
                "Too Many Requests"
            };
            let ctype = if status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            };
            let retry = if status == 429 {
                "Retry-After: 1\r\n"
            } else {
                ""
            };
            let resp = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\n{retry}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.shutdown().await.ok();
        }
    });
    (format!("http://{addr}"), seen)
}

fn target(name: &str, url: &str) -> Target {
    Target::new(
        Arc::new(ProviderConfig {
            name: name.into(),
            base_url: url.into(),
            api_key: Some("test-key".into()),
            headers: vec![],
            stream_usage: false,
        }),
        "m",
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn loop_falls_back_runs_tools_and_finishes() {
    let busy_script = vec![(429, r#"{"error":"rate limited"}"#.to_string())];
    let (busy_url, _) = fake_server(busy_script).await;

    let good_script = vec![
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"content":"Creating the file."}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"write_file","arguments":"{\"path\":\"hello.py\","}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"content\":\"print('helo')\\n\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            ]),
        ),
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c2","function":{"name":"edit_file","arguments":"{\"path\":\"hello.py\",\"old_string\":\"helo\",\"new_string\":\"hello\"}"}},{"index":1,"id":"c3","function":{"name":"read_file","arguments":"not json"}}]},"finish_reason":"tool_calls"}]}"#,
            ]),
        ),
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"content":"Done: hello.py prints hello."},"finish_reason":"stop"}],"usage":{"prompt_tokens":120,"completion_tokens":9}}"#,
            ]),
        ),
    ];
    let (good_url, seen) = fake_server(good_script).await;

    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let router = Router::new(vec![target("busy", &busy_url), target("good", &good_url)]);
    let mut agent = Agent::new(
        router,
        tools,
        Box::new(AlwaysYes),
        ApprovalMode::Ask,
        10,
        "system".into(),
    );

    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let summary = agent.run_turn("make hello.py", &mut sink).await.unwrap();

    // The file was created and then fixed by the edit.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("hello.py")).unwrap(),
        "print('hello')\n"
    );

    // Fallback happened once, from the busy provider.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Fallback { from, .. } if from == "busy/m")));
    assert_eq!(summary.steps, 3);
    assert_eq!(summary.tool_calls, 3);
    assert_eq!(summary.usage.prompt_tokens, 120);

    // History: system, user, assistant(call), tool, assistant(2 calls), tool, tool, assistant(final).
    let roles: Vec<Role> = agent.session.messages.iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        vec![
            Role::System,
            Role::User,
            Role::Assistant,
            Role::Tool,
            Role::Assistant,
            Role::Tool,
            Role::Tool,
            Role::Assistant
        ]
    );
    // The malformed call got an explanatory error, not a crash.
    let bad = agent
        .session
        .messages
        .iter()
        .find(|m| m.tool_call_id.as_deref() == Some("c3"))
        .unwrap();
    assert!(bad
        .content
        .as_ref()
        .unwrap()
        .contains("not a valid JSON object"));

    // Requests carried tools, and later requests carried tool results back.
    let reqs = seen.lock().unwrap();
    assert_eq!(reqs.len(), 3);
    assert!(reqs[0]["tools"].as_array().unwrap().len() >= 4);
    assert_eq!(reqs[0]["stream"], true);
    let last_msgs = reqs[2]["messages"].as_array().unwrap();
    assert!(last_msgs
        .iter()
        .any(|m| m["role"] == "tool" && m["tool_call_id"] == "c2"));
    drop(reqs);

    // Undo reverts both file changes of the turn.
    let r = agent.undo_last_turn().unwrap().unwrap();
    assert_eq!(r.removed + r.restored, 2);
    assert!(!dir.path().join("hello.py").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_providers_failing_is_a_clear_error() {
    let (u1, _) = fake_server(vec![(429, "{}".into())]).await;
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(
        Router::new(vec![target("only", &u1)]),
        tools,
        Box::new(AlwaysYes),
        ApprovalMode::Ask,
        5,
        "s".into(),
    );
    let mut sink = |_e: AgentEvent| {};
    let err = agent
        .run_turn("hi", &mut sink)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("every model failed") && err.contains("only/m"),
        "{err}"
    );
}

struct NeverAsked;
impl Approver for NeverAsked {
    fn approve(&self, tool: &str, _: &str) -> Decision {
        panic!("approval should not be needed for {tool}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rules_block_denied_calls_and_skip_prompts_for_allowed_ones() {
    let script = vec![
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"d1","function":{"name":"shell","arguments":"{\"command\":\"git push --force origin main\"}"}},{"index":1,"id":"w1","function":{"name":"write_file","arguments":"{\"path\":\"notes.txt\",\"content\":\"hi\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            ]),
        ),
        (
            200,
            sse(&[r#"{"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}"#]),
        ),
    ];
    let (url, _) = fake_server(script).await;
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(
        Router::new(vec![target("p", &url)]),
        tools,
        Box::new(NeverAsked),
        ApprovalMode::Ask,
        5,
        "s".into(),
    );
    agent.set_rules(
        Rules::new(
            &["write_file(notes.txt)".into()],
            &["shell(*git push --force*)".into()],
        )
        .unwrap(),
    );
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    agent.run_turn("go", &mut sink).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
        "hi"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Blocked { name, .. } if name == "shell")));
    let blocked = agent
        .session
        .messages
        .iter()
        .find(|m| m.tool_call_id.as_deref() == Some("d1"))
        .unwrap();
    assert!(blocked
        .content
        .as_ref()
        .unwrap()
        .contains("Blocked by the rule"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn long_history_is_summarized_before_the_next_request() {
    let script = vec![
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"content":"User fixed bug A in a.py."},"finish_reason":"stop"}]}"#,
            ]),
        ),
        (
            200,
            sse(&[
                r#"{"choices":[{"delta":{"content":"Next step done."},"finish_reason":"stop"}]}"#,
            ]),
        ),
    ];
    let (url, seen) = fake_server(script).await;
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(
        Router::new(vec![target("p", &url)]),
        tools,
        Box::new(AlwaysYes),
        ApprovalMode::Ask,
        5,
        "s".into(),
    );
    // Tool specs alone are ~2.5K tokens; leave room for only a little history.
    agent.context_tokens = 4000;
    let filler = "word ".repeat(1500);
    for i in 0..4 {
        agent
            .session
            .messages
            .push(Message::user(format!("task {i}: {filler}")));
        agent.session.messages.push(Message::assistant(
            Some(format!("reply {i}: {filler}")),
            vec![],
        ));
    }
    let mut sink = |_e: AgentEvent| {};
    agent
        .run_turn("and now the next step", &mut sink)
        .await
        .unwrap();

    let reqs = seen.lock().unwrap();
    // First request is the summary call: no tools, our summary prompt.
    assert!(reqs[0].get("tools").is_none());
    assert_eq!(reqs[0]["messages"][0]["content"], SUMMARY_PROMPT);
    // The real request carries the summary instead of the old turns.
    let msgs = reqs[1]["messages"].as_array().unwrap();
    assert!(msgs[1]["content"]
        .as_str()
        .unwrap()
        .starts_with(SUMMARY_MARKER));
    assert!(
        msgs.len() < 10,
        "history was not shortened: {} messages",
        msgs.len()
    );
    assert!(msgs.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("next step"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn waits_out_a_short_rate_limit_instead_of_failing() {
    // Like Groq's free tier: one 429 with Retry-After: 1, then success.
    let script = vec![
        (
            429,
            r#"{"error":{"message":"Rate limit reached ... tokens per minute (TPM)"}}"#.to_string(),
        ),
        (
            200,
            sse(&[r#"{"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"#]),
        ),
    ];
    let (url, _) = fake_server(script).await;
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(
        Router::new(vec![target("groqlike", &url)]),
        tools,
        Box::new(AlwaysYes),
        ApprovalMode::Ask,
        5,
        "s".into(),
    );
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let started = std::time::Instant::now();
    agent.run_turn("hi", &mut sink).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::Waiting(s) if *s >= 1 && *s <= 3)),
        "{events:?}"
    );
    assert!(started.elapsed() >= std::time::Duration::from_secs(1));
    assert_eq!(
        agent.session.messages.last().unwrap().content.as_deref(),
        Some("done")
    );
}

/// A fake provider that answers by looking at each request, and serves
/// connections concurrently (needed for parallel sub-agents). Tracks the most
/// sub-agent requests in flight at once.
async fn smart_server() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_seen = Arc::new(AtomicUsize::new(0));
    let max_out = max_seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let in_flight = in_flight.clone();
            let max_seen = max_seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                let (head_end, len) = loop {
                    let n = sock.read(&mut tmp).await.unwrap();
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
                let msgs = req["messages"].as_array().unwrap();
                let system = msgs[0]["content"].as_str().unwrap_or("");
                let has_tool_results = msgs.iter().any(|m| m["role"] == "tool");
                let body = if system.contains("You are a sub-agent") {
                    let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    if !has_tool_results {
                        // A sub-agent trying to write must be refused.
                        sse(&[
                            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"w","function":{"name":"write_file","arguments":"{\"path\":\"hacked.txt\",\"content\":\"x\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                        ])
                    } else {
                        let job = msgs.iter().find(|m| m["role"] == "user").unwrap()["content"]
                            .as_str()
                            .unwrap()
                            .to_string();
                        let text = serde_json::to_string(&format!("REPORT for {job}")).unwrap();
                        sse(&[&format!(
                            r#"{{"choices":[{{"delta":{{"content":{text}}},"finish_reason":"stop"}}]}}"#
                        )])
                    }
                } else if msgs.last().unwrap()["role"] == "user" {
                    sse(&[
                        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"t1","function":{"name":"task","arguments":"{\"description\":\"alpha\",\"prompt\":\"alpha job\"}"}},{"index":1,"id":"t2","function":{"name":"task","arguments":"{\"description\":\"beta\",\"prompt\":\"beta job\"}"}}]},"finish_reason":"tool_calls"}]}"#,
                    ])
                } else {
                    sse(&[
                        r#"{"choices":[{"delta":{"content":"combined"},"finish_reason":"stop"}]}"#,
                    ])
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
                sock.shutdown().await.ok();
            });
        }
    });
    (format!("http://{addr}"), max_out)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn task_calls_run_read_only_subagents_in_parallel() {
    use oblivion_core::SubagentFactory;
    let (url, max_parallel) = smart_server().await;
    let dir = tempfile::tempdir().unwrap();
    let router = Router::new(vec![target("p", &url)]);
    let factory = SubagentFactory {
        router: router.share(),
        root: dir.path().to_path_buf(),
        shell: Shell::Sh,
        search: oblivion_tools::SearchBackend::DuckDuckGo,
        skills: None,
        read_roots: vec![],
        base_prompt: "base".into(),
        context_tokens: 24_000,
        max_steps: 5,
        rules: Rules::default(),
        prefer: String::new(),
    };
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(
        router,
        tools,
        Box::new(NeverAsked),
        ApprovalMode::Ask,
        5,
        "main".into(),
    );
    agent.set_subagents(factory);

    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    agent
        .run_turn("research two things", &mut sink)
        .await
        .unwrap();

    // Both sub-agents ran at the same time.
    assert_eq!(max_parallel.load(std::sync::atomic::Ordering::SeqCst), 2);
    // Their write attempt was refused: read-only.
    assert!(!dir.path().join("hacked.txt").exists());
    // Each report came back as the matching tool result, in order.
    let r1 = agent
        .session
        .messages
        .iter()
        .find(|m| m.tool_call_id.as_deref() == Some("t1"))
        .unwrap();
    let r2 = agent
        .session
        .messages
        .iter()
        .find(|m| m.tool_call_id.as_deref() == Some("t2"))
        .unwrap();
    assert!(
        r1.content
            .as_ref()
            .unwrap()
            .contains("REPORT for alpha job"),
        "{:?}",
        r1.content
    );
    assert!(
        r2.content.as_ref().unwrap().contains("REPORT for beta job"),
        "{:?}",
        r2.content
    );
    assert_eq!(
        agent.session.messages.last().unwrap().content.as_deref(),
        Some("combined")
    );
    // Progress from sub-agents was forwarded, labelled.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Sub { label, .. } if label == "beta")));
    // The parent offers the task tool; nothing in the sub-agent list can write.
    assert!(oblivion_core::subagent::SUBAGENT_TOOLS.iter().all(|t| ![
        "write_file",
        "edit_file",
        "shell",
        "mcp"
    ]
    .contains(t)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_repeated_calls_are_refused_then_the_turn_stops() {
    let same = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"r","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#;
    let script: Vec<(u16, String)> = (0..8).map(|_| (200, sse(&[same]))).collect();
    let (url, seen) = fake_server(script).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(Router::new(vec![target("p", &url)]), tools, Box::new(AlwaysYes), ApprovalMode::Ask, 20, "s".into());
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    agent.run_turn("loop please", &mut sink).await.unwrap();
    // 2 real reads, then 3 refusals, then the turn stops (5 requests, not 20).
    assert_eq!(seen.lock().unwrap().len(), 5);
    let refused = agent.session.messages.iter().filter(|m| m.content.as_deref().is_some_and(|c| c.contains("exact read_file call"))).count();
    assert_eq!(refused, 3);
    assert!(events.iter().any(|e| matches!(e, AgentEvent::Notice(n) if n.starts_with("Stopped: the model kept repeating"))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn web_calls_past_the_budget_are_refused() {
    let search = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"w","function":{"name":"web_search","arguments":"{\"query\":\"rust\"}"}}]},"finish_reason":"tool_calls"}]}"#;
    let done = r#"{"choices":[{"delta":{"content":"answer"},"finish_reason":"stop"}]}"#;
    let (url, seen) = fake_server(vec![(200, sse(&[search])), (200, sse(&[done]))]).await;
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(Router::new(vec![target("p", &url)]), tools, Box::new(AlwaysYes), ApprovalMode::Ask, 20, "s".into());
    agent.max_web_calls = 0;
    let mut sink = |_e: AgentEvent| {};
    agent.run_turn("research", &mut sink).await.unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(agent.session.messages.iter().any(|m| m.content.as_deref().is_some_and(|c| c.contains("web budget for this request"))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_last_step_has_no_tools_and_asks_for_an_answer() {
    let read = r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"r","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]},"finish_reason":"tool_calls"}]}"#;
    let done = r#"{"choices":[{"delta":{"content":"final answer"},"finish_reason":"stop"}]}"#;
    let (url, seen) = fake_server(vec![(200, sse(&[read])), (200, sse(&[done]))]).await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();
    let tools = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
    let mut agent = Agent::new(Router::new(vec![target("p", &url)]), tools, Box::new(AlwaysYes), ApprovalMode::Ask, 2, "s".into());
    let mut sink = |_e: AgentEvent| {};
    let summary = agent.run_turn("go", &mut sink).await.unwrap();
    let reqs = seen.lock().unwrap();
    assert!(reqs[0].get("tools").is_some());
    assert!(reqs[1].get("tools").is_none(), "last step must not offer tools");
    assert!(reqs[1].to_string().contains("This is the last step"));
    assert!(!summary.hit_step_limit);
    assert_eq!(agent.session.messages.last().unwrap().content.as_deref(), Some("final answer"));
}
