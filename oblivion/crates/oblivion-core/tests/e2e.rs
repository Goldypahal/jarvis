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
