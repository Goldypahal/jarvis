//! Test helpers: a scripted fake model server and an agent wired to the
//! interfaces' worker, used by the web and terminal UI tests.
#![cfg(test)]

use std::sync::{Arc, Mutex};

use oblivion_core::{Agent, ApprovalMode};
use oblivion_mcp::McpHub;
use oblivion_providers::{ProviderConfig, Router, Target};
use oblivion_skills::SkillSet;
use oblivion_tools::{Shell, ToolBox};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::app::{self, Controls, UiEvent, Worker};

pub fn sse(chunks: &[&str]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

/// Serves scripted responses in order; records each request body.
pub async fn fake_server(
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

pub fn target(name: &str, url: &str) -> Target {
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

pub fn tool_call(id: &str, name: &str, args: &str) -> String {
    let args = serde_json::to_string(args).unwrap();
    format!(
        r#"{{"choices":[{{"delta":{{"tool_calls":[{{"index":0,"id":"{id}","function":{{"name":"{name}","arguments":{args}}}}}]}},"finish_reason":"tool_calls"}}]}}"#
    )
}

pub fn text(t: &str) -> String {
    format!(
        r#"{{"choices":[{{"delta":{{"content":{}}},"finish_reason":"stop"}}]}}"#,
        serde_json::to_string(t).unwrap()
    )
}

/// A worker whose agent talks to `url`, working in `dir`, asking before changes.
pub fn worker(
    url: &str,
    dir: &std::path::Path,
) -> (
    Worker,
    Controls,
    tokio::sync::mpsc::UnboundedReceiver<UiEvent>,
) {
    let (tx, rx) = app::channels();
    let approvals = app::Approvals::new(tx.clone());
    let tools = ToolBox::new(dir, Shell::Sh, dir.join(".snap"));
    let agent = Agent::new(
        Router::new(vec![target("fake", url)]),
        tools,
        approvals.approver(),
        ApprovalMode::Ask,
        10,
        "system".into(),
    );
    let (w, c) = Worker::new(
        agent,
        Arc::new(SkillSet::discover(&[])),
        Arc::new(McpHub::new(vec![])),
        false,
        tx,
        approvals,
    );
    (w, c, rx)
}

#[allow(dead_code)]
pub type Seen = Arc<Mutex<Vec<serde_json::Value>>>;

/// Declines open approvals when a test ends (even by panicking), so the
/// runtime is not left waiting for an answer.
pub struct Release(pub Controls);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
