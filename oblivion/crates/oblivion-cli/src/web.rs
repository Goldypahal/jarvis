//! `oblivion --web`: the agent in a local web page.
//!
//! Safety: the server listens on 127.0.0.1 only, and every API call must carry a
//! random token (in the page URL), so other websites open in the same browser
//! cannot drive the agent. Events stream to the page with Server-Sent Events;
//! a page that reconnects first receives everything so far.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, Stream, StreamExt};
use oblivion_core::Decision;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::{Controls, UiEvent};

const PAGE: &str = include_str!("../web/index.html");
/// Events kept for pages that connect later (streamed text is merged first).
const HISTORY_LIMIT: usize = 4_000;

#[derive(Serialize, Clone)]
struct Envelope {
    seq: u64,
    #[serde(flatten)]
    event: UiEvent,
}

/// Every event so far (for late or reconnecting pages) plus a live feed.
pub struct Bus {
    seq: AtomicU64,
    history: Mutex<Vec<Envelope>>,
    live: broadcast::Sender<String>,
}

impl Default for Bus {
    fn default() -> Self {
        Self {
            seq: AtomicU64::new(0),
            history: Mutex::new(Vec::new()),
            live: broadcast::channel(4096).0,
        }
    }
}

impl Bus {
    pub fn publish(&self, event: UiEvent) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let env = Envelope { seq, event };
        let json = serde_json::to_string(&env).unwrap_or_default();
        {
            let mut h = self.history.lock().unwrap();
            match (&env.event, h.last_mut()) {
                // Streamed text arrives a few characters at a time; store it merged.
                (
                    UiEvent::Text { text },
                    Some(Envelope {
                        event: UiEvent::Text { text: prev },
                        seq: last_seq,
                    }),
                ) => {
                    prev.push_str(text);
                    *last_seq = seq;
                }
                // Only the latest state and task list matter.
                (UiEvent::State(_), _) => {
                    h.retain(|e| !matches!(e.event, UiEvent::State(_)));
                    h.push(env.clone());
                }
                (UiEvent::Todos { .. }, _) => {
                    h.retain(|e| !matches!(e.event, UiEvent::Todos { .. }));
                    h.push(env.clone());
                }
                _ => h.push(env.clone()),
            }
            if h.len() > HISTORY_LIMIT {
                let extra = h.len() - HISTORY_LIMIT;
                h.drain(..extra);
            }
        }
        let _ = self.live.send(json);
    }

    fn backlog(&self) -> Vec<String> {
        self.history
            .lock()
            .unwrap()
            .iter()
            .map(|e| serde_json::to_string(e).unwrap_or_default())
            .collect()
    }
}

#[derive(Clone)]
pub struct WebState {
    pub token: Arc<str>,
    pub controls: Controls,
    pub bus: Arc<Bus>,
}

impl WebState {
    fn authorized(&self, headers: &HeaderMap, query: &HashMap<String, String>) -> bool {
        let given = headers
            .get("x-oblivion-token")
            .and_then(|v| v.to_str().ok())
            .or(query.get("t").map(String::as_str))
            .unwrap_or("");
        constant_time_eq(given.as_bytes(), self.token.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 32 random bytes from the operating system, as hex.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random number generator failed");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/events", get(events))
        .route("/send", post(send))
        .route("/approve", post(approve))
        .route("/cancel", post(cancel))
        .with_state(state)
}

async fn page() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            // The page only talks to this server; no outside scripts, frames or images.
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'",
            ),
        ],
        Html(PAGE),
    )
        .into_response()
}

async fn events(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    // Subscribe before taking the backlog: an event may then arrive twice, and the
    // page drops anything with a sequence number it has already seen.
    let rx = st.bus.live.subscribe();
    let backlog = stream::iter(st.bus.backlog()).map(|j| Ok(Event::default().data(j)));
    let live = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(j) => return Some((Ok(Event::default().data(j)), rx)),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(backlog.chain(live)).keep_alive(KeepAlive::default()))
}

#[derive(Deserialize)]
struct SendBody {
    text: String,
}

async fn send(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<SendBody>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    if body.text.trim().is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    st.controls.send(body.text);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct ApproveBody {
    id: u64,
    answer: String,
}

async fn approve(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<ApproveBody>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    let decision = match body.answer.as_str() {
        "yes" => Decision::Yes,
        "always" => Decision::Always,
        "no" => Decision::No,
        _ => return StatusCode::BAD_REQUEST,
    };
    if st.controls.approvals.answer(body.id, decision) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::GONE
    }
}

async fn cancel(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    st.controls.cancel();
    StatusCode::NO_CONTENT
}

/// Copy worker events onto the bus.
pub async fn pump(mut rx: UnboundedReceiver<UiEvent>, bus: Arc<Bus>) {
    while let Some(e) = rx.recv().await {
        bus.publish(e);
    }
}

/// Bind 127.0.0.1:port, or any free port if that one is taken.
pub async fn bind(port: u16) -> std::io::Result<(tokio::net::TcpListener, SocketAddr)> {
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(_) => tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?,
    };
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// Open a URL in the default browser (best effort).
pub fn open_browser(url: &str) {
    let result = if cfg!(windows) {
        // `start` treats the first quoted argument as a window title.
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    let _ = result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_merges_text_and_keeps_one_state() {
        let bus = Bus::default();
        bus.publish(UiEvent::User { text: "hi".into() });
        bus.publish(UiEvent::Text { text: "Hel".into() });
        bus.publish(UiEvent::Text { text: "lo".into() });
        bus.publish(UiEvent::Todos { items: vec![] });
        bus.publish(UiEvent::Todos { items: vec![] });
        bus.publish(UiEvent::TextEnd);
        let b = bus.backlog();
        assert_eq!(b.len(), 4, "{b:?}");
        assert!(b[1].contains(r#""type":"text","text":"Hello""#), "{}", b[1]);
        assert!(
            b[1].contains(r#""seq":3"#),
            "merged text keeps the latest seq"
        );
    }

    #[test]
    fn tokens_are_long_and_compared_exactly() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token());
        assert!(constant_time_eq(a.as_bytes(), a.as_bytes()));
        assert!(!constant_time_eq(a.as_bytes(), &a.as_bytes()[1..]));
    }
}

#[cfg(test)]
mod live {
    use super::*;
    use crate::testkit::{fake_server, sse, text, tool_call, worker};
    use futures_util::StreamExt;
    use serde_json::Value;
    use std::time::Duration;

    async fn next_of(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Value>, ty: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let e = rx.recv().await.expect("event stream ended");
                if e["type"] == ty {
                    return e;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {ty} event"))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn page_runs_a_request_with_an_approval() {
        let write = tool_call(
            "w1",
            "write_file",
            r#"{"path":"hello.txt","content":"hi there"}"#,
        );
        let (url, _seen) = fake_server(vec![
            (200, sse(&[&write])),
            (200, sse(&[&text("Created hello.txt.")])),
        ])
        .await;
        let dir = tempfile::tempdir().unwrap();
        let (w, controls, events) = worker(&url, dir.path());
        let _release = crate::testkit::Release(controls.clone());

        let bus = Arc::new(Bus::default());
        let token = new_token();
        let (listener, addr) = bind(0).await.unwrap();
        let state = WebState {
            token: token.clone().into(),
            controls: controls.clone(),
            bus: bus.clone(),
        };
        tokio::spawn(pump(events, bus));
        tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        tokio::spawn(w.run());
        let base = format!("http://{addr}");
        let http = reqwest::Client::new();

        // The page itself, with a strict content policy.
        let page = http.get(&base).send().await.unwrap();
        assert!(page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'self'"));
        assert!(page
            .text()
            .await
            .unwrap()
            .contains("<title>Oblivion</title>"));

        // No token, no access.
        let r = http
            .post(format!("{base}/send"))
            .json(&serde_json::json!({"text":"x"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
        assert_eq!(
            http.get(format!("{base}/events"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );

        // Stream events like the page does.
        let es = http
            .get(format!("{base}/events?t={token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(es.status(), 200);
        let (etx, mut erx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        tokio::spawn(async move {
            let mut body = es.bytes_stream();
            let mut buf = String::new();
            while let Some(Ok(chunk)) = body.next().await {
                buf.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(i) = buf.find("\n\n") {
                    let frame: String = buf.drain(..i + 2).collect();
                    for line in frame.lines() {
                        if let Some(d) = line.strip_prefix("data: ").or(line.strip_prefix("data:"))
                        {
                            if let Ok(v) = serde_json::from_str::<Value>(d) {
                                let _ = etx.send(v);
                            }
                        }
                    }
                }
            }
        });
        let st = next_of(&mut erx, "state").await;
        assert_eq!(st["approval"], "ask");

        let r = http
            .post(format!("{base}/send"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"text":"make hello.txt"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(next_of(&mut erx, "user").await["text"], "make hello.txt");
        let start = next_of(&mut erx, "tool_start").await;
        assert_eq!(start["name"], "write_file");
        assert_eq!(start["detail"], "+ hi there");
        let ask = next_of(&mut erx, "approval").await;
        assert!(ask["summary"].as_str().unwrap().contains("hello.txt"));
        assert!(
            !dir.path().join("hello.txt").exists(),
            "nothing written before approval"
        );

        let id = ask["id"].as_u64().unwrap();
        let r = http
            .post(format!("{base}/approve"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"id": id, "answer": "yes"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);
        assert_eq!(next_of(&mut erx, "approval_done").await["answer"], "yes");
        let end = next_of(&mut erx, "tool_end").await;
        assert_eq!(end["is_error"], false);
        assert_eq!(
            next_of(&mut erx, "text").await["text"],
            "Created hello.txt."
        );
        let done = next_of(&mut erx, "turn_end").await;
        assert_eq!(done["outcome"], "done");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("hello.txt")).unwrap(),
            "hi there"
        );

        // Answering twice is refused; slash commands come back as info.
        let r = http
            .post(format!("{base}/approve"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"id": id, "answer": "yes"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 410);
        http.post(format!("{base}/send"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"text":"/undo"}))
            .send()
            .await
            .unwrap();
        let info = next_of(&mut erx, "info").await;
        assert!(
            info["text"]
                .as_str()
                .unwrap()
                .contains("removed 1 new file"),
            "{info}"
        );
        assert!(!dir.path().join("hello.txt").exists());

        // A page that connects late gets the whole conversation.
        let late = http
            .get(format!("{base}/events?t={token}"))
            .send()
            .await
            .unwrap();
        let mut body = late.bytes_stream();
        let mut got = String::new();
        while !got.contains("Created hello.txt.") {
            let chunk = tokio::time::timeout(Duration::from_secs(5), body.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            got.push_str(&String::from_utf8_lossy(&chunk));
        }
        assert!(got.contains(r#""type":"user","text":"make hello.txt""#));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancel_declines_a_waiting_approval() {
        let write = tool_call("w1", "write_file", r#"{"path":"x.txt","content":"x"}"#);
        let (url, _seen) = fake_server(vec![(200, sse(&[&write]))]).await;
        let dir = tempfile::tempdir().unwrap();
        let (w, controls, mut events) = worker(&url, dir.path());
        let _release = crate::testkit::Release(controls.clone());
        tokio::spawn(w.run());
        controls.send("write x");
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Some(UiEvent::Approval { .. }) = events.recv().await {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(controls.busy());
        controls.cancel();
        let end = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Some(e @ UiEvent::TurnEnd { .. }) = events.recv().await {
                    return e;
                }
            }
        })
        .await
        .unwrap();
        assert!(
            matches!(end, UiEvent::TurnEnd { ref outcome, .. } if outcome == "cancelled"),
            "{end:?}"
        );
        assert!(!controls.busy());
        assert!(!dir.path().join("x.txt").exists());
    }
}
