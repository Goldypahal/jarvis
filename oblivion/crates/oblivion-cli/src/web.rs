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

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
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
                (UiEvent::SessionResumed { .. }, _) => {
                    h.clear();
                    h.push(env.clone());
                }
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
    pub store: Arc<oblivion_store::Store>,
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
        .route("/api/system/dashboard", get(get_dashboard))
        .route("/api/system/hardware", get(get_hardware))
        .route(
            "/api/system/agents-md",
            get(get_agents_md).post(apply_agents_md),
        )
        .route("/api/system/agents-md/apply", post(apply_agents_md))
        .route(
            "/api/memory",
            get(list_or_search_memories).post(create_memory),
        )
        .route("/api/memory/{id}", put(update_memory).delete(delete_memory))
        .route("/api/memory/forget", post(forget_memories_topic))
        .route("/api/memory/export", get(export_memories))
        .route(
            "/api/sessions",
            get(list_or_search_sessions).post(create_or_switch_session),
        )
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/rename", post(rename_session))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route(
            "/api/sessions/{id}/workspace",
            post(update_session_workspace),
        )
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

async fn list_or_search_memories(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q_map): Query<HashMap<String, String>>,
) -> Result<Json<Vec<oblivion_store::Memory>>, StatusCode> {
    if !st.authorized(&headers, &q_map) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let query_str = q_map.get("q").map(String::as_str).unwrap_or("").trim();
    let mem_type = q_map.get("type").filter(|t| !t.is_empty()).cloned();
    let project = q_map.get("project").filter(|p| !p.is_empty()).cloned();
    let limit = q_map
        .get("limit")
        .and_then(|l| l.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, 200);

    let filter = oblivion_store::MemoryFilter {
        mem_type,
        project,
        include_global: true,
        ..Default::default()
    };

    let memories = if query_str.is_empty() {
        st.store
            .list_memories(Some(&filter), limit)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        st.store
            .search_memories(query_str, Some(&filter), limit)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    Ok(Json(memories))
}

#[derive(Deserialize)]
struct CreateMemoryBody {
    #[serde(rename = "type")]
    mem_type: Option<String>,
    text: String,
    importance: Option<u32>,
    project: Option<String>,
}

async fn create_memory(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<CreateMemoryBody>,
) -> Result<(StatusCode, Json<oblivion_store::Memory>), StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let text = body.text.trim();
    if text.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mem_type = body.mem_type.as_deref().unwrap_or("fact");
    let importance = body.importance.unwrap_or(3).clamp(1, 5);

    let mut new_mem = oblivion_store::NewMemory::new(mem_type, text, importance);
    if let Some(ref proj) = body.project {
        if !proj.trim().is_empty() {
            new_mem = new_mem.with_project(proj);
        }
    }

    let created = st
        .store
        .add_memory(&new_mem)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(embedder) = st.store.embedder() {
        if let Ok(vec) = embedder.embed(&created.text).await {
            let _ = st
                .store
                .save_memory_embedding(created.id, embedder.model_name(), &vec);
        }
    }

    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
struct UpdateMemoryBody {
    text: Option<String>,
    #[serde(rename = "type")]
    mem_type: Option<String>,
    importance: Option<u32>,
}

async fn update_memory(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateMemoryBody>,
) -> Result<Json<oblivion_store::Memory>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let updated = st
        .store
        .update_memory(
            id,
            body.text.as_deref(),
            body.mem_type.as_deref(),
            body.importance,
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if let Some(embedder) = st.store.embedder() {
        if let Ok(vec) = embedder.embed(&updated.text).await {
            let _ = st
                .store
                .save_memory_embedding(updated.id, embedder.model_name(), &vec);
        }
    }

    Ok(Json(updated))
}

async fn delete_memory(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let deleted = st
        .store
        .delete_memory(id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

#[derive(Deserialize)]
struct ForgetTopicBody {
    query: String,
}

#[derive(Serialize)]
struct ForgetTopicResponse {
    deleted_count: usize,
    query: String,
}

async fn forget_memories_topic(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<ForgetTopicBody>,
) -> Result<Json<ForgetTopicResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let query = body.query.trim();
    if query.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let matches = st
        .store
        .search_memories(query, None, 100)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut count = 0;
    for m in matches {
        if let Ok(true) = st.store.delete_memory(m.id) {
            count += 1;
        }
    }

    Ok(Json(ForgetTopicResponse {
        deleted_count: count,
        query: query.to_string(),
    }))
}

async fn export_memories(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q_map): Query<HashMap<String, String>>,
) -> Result<Response, StatusCode> {
    if !st.authorized(&headers, &q_map) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let memories = st
        .store
        .list_memories(None, 1000)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let format = q_map.get("format").map(String::as_str).unwrap_or("json");
    if format == "markdown" || format == "md" {
        let mut md = String::from("# Jarvis Long-Term Memories\n\n");
        let categories = [
            "user_pref",
            "project_fact",
            "recipe",
            "fact",
            "past_outcome",
        ];
        for cat in categories {
            let cat_items: Vec<_> = memories.iter().filter(|m| m.mem_type == cat).collect();
            if !cat_items.is_empty() {
                md.push_str(&format!("## {cat}\n\n"));
                for item in cat_items {
                    let proj = item
                        .project
                        .as_deref()
                        .map(|p| format!(" (project: `{p}`)"))
                        .unwrap_or_default();
                    md.push_str(&format!(
                        "- [id:{}] **(imp {})** {}{}\n",
                        item.id, item.importance, item.text, proj
                    ));
                }
                md.push('\n');
            }
        }
        Ok((
            [
                (header::CONTENT_TYPE, "text/markdown; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"memories.md\"",
                ),
            ],
            md,
        )
            .into_response())
    } else {
        let json_str = serde_json::to_string_pretty(&memories)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok((
            [
                (header::CONTENT_TYPE, "application/json; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"memories.json\"",
                ),
            ],
            json_str,
        )
            .into_response())
    }
}

async fn list_or_search_sessions(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Vec<oblivion_core::SessionInfo>>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let limit = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50);
    let sessions = if let Some(search_q) = q.get("q").filter(|s| !s.trim().is_empty()) {
        st.store.search_sessions(search_q, limit)
    } else {
        st.store.list_sessions(limit)
    }
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(sessions))
}

async fn get_session(
    State(st): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<oblivion_core::Session>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let session = st
        .store
        .load_session(&id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(session))
}

#[derive(Deserialize)]
struct CreateSessionBody {
    workspace: Option<String>,
}

async fn create_or_switch_session(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<CreateSessionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let cmd = match &body.workspace {
        Some(w) if !w.trim().is_empty() => format!("/new {}", w.trim()),
        _ => "/new".to_string(),
    };
    st.controls.send(cmd);
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "status": "created", "workspace": body.workspace })),
    ))
}

#[derive(Deserialize)]
struct RenameSessionBody {
    title: String,
}

async fn rename_session(
    State(st): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<RenameSessionBody>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    if body.title.trim().is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    match st.store.rename_session(&id, &body.title) {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn resume_session(
    State(st): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<oblivion_core::Session>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let session = st
        .store
        .load_session(&id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    st.controls.send(format!("/resume {id}"));
    Ok(Json(session))
}

async fn delete_session(
    State(st): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    match st.store.delete_session(&id) {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Deserialize)]
struct WorkspaceBody {
    workspace: String,
}

async fn update_session_workspace(
    State(st): State<WebState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<WorkspaceBody>,
) -> StatusCode {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED;
    }
    let p = std::path::Path::new(body.workspace.trim());
    if !p.is_dir() {
        return StatusCode::BAD_REQUEST;
    }
    match st.store.update_session_workspace(&id, p) {
        Ok(true) => {
            st.controls.send(format!("/workspace {}", p.display()));
            StatusCode::OK
        }
        Ok(false) => StatusCode::NOT_FOUND,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardTask {
    pub text: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub modified: u64,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardEngine {
    pub version: String,
    pub status: String,
    pub model: String,
    pub workspace: String,
    #[serde(default)]
    pub hardware: Option<oblivion_core::HardwareInfo>,
    #[serde(default)]
    pub recommendation: Option<oblivion_core::ModelRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    pub usage_today: Vec<oblivion_store::UsageRow>,
    pub tasks: Vec<DashboardTask>,
    pub recent_files: Vec<DashboardFile>,
    pub memories_count: usize,
    pub memories_by_type: HashMap<String, usize>,
    pub sessions_count: usize,
    pub engine: DashboardEngine,
}

async fn get_dashboard(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<DashboardData>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let usage_today = st.store.usage_today().unwrap_or_default();
    let memories_count = st.store.count_memories(None).unwrap_or(0) as usize;
    let mut memories_by_type = HashMap::new();
    if let Ok(mems) = st.store.list_memories(None, 1000) {
        for m in mems {
            *memories_by_type.entry(m.mem_type).or_insert(0) += 1;
        }
    }
    let sessions_count = st.store.list_sessions(1000).map(|s| s.len()).unwrap_or(0);

    let mut tasks = Vec::new();
    let mut model_label = "groq:llama-3.3-70b-versatile".to_string();
    let mut workspace_path = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    {
        let h = st.bus.history.lock().unwrap();
        for env in h.iter() {
            match &env.event {
                UiEvent::Todos { items } => {
                    tasks = items
                        .iter()
                        .map(|t| DashboardTask {
                            text: t.text.clone(),
                            status: t.status.clone(),
                        })
                        .collect();
                }
                UiEvent::Model { label } => {
                    model_label = label.clone();
                }
                UiEvent::State(s) => {
                    workspace_path = s.root.clone();
                }
                UiEvent::SessionResumed { workspace, .. } => {
                    workspace_path = workspace.clone();
                }
                _ => {}
            }
        }
    }

    let status = if st.controls.busy() {
        "busy".to_string()
    } else {
        "ready".to_string()
    };

    let hw = oblivion_core::detect_hardware();
    let rec = oblivion_core::recommend_model(&hw);
    let engine = DashboardEngine {
        version: env!("CARGO_PKG_VERSION").to_string(),
        status,
        model: model_label,
        workspace: workspace_path.clone(),
        hardware: Some(hw),
        recommendation: Some(rec),
    };

    let recent_files = scan_recent_files(std::path::Path::new(&workspace_path), 20);

    Ok(Json(DashboardData {
        usage_today,
        tasks,
        recent_files,
        memories_count,
        memories_by_type,
        sessions_count,
        engine,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareResponse {
    pub hardware: oblivion_core::HardwareInfo,
    pub recommendation: oblivion_core::ModelRecommendation,
}

async fn get_hardware(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<HardwareResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let hardware = oblivion_core::detect_hardware();
    let recommendation = oblivion_core::recommend_model(&hardware);
    Ok(Json(HardwareResponse {
        hardware,
        recommendation,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsMdResponse {
    pub file: String,
    pub exists: bool,
    pub content: String,
    pub loaded_at_start: Option<String>,
    pub proposed_additions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsMdApplyResponse {
    pub status: String,
    pub file: String,
    pub added_count: usize,
    pub additions: Vec<String>,
}

async fn get_agents_md(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<AgentsMdResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut workspace_path = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    {
        let h = st.bus.history.lock().unwrap();
        for env in h.iter() {
            match &env.event {
                UiEvent::State(s) => {
                    workspace_path = s.root.clone();
                }
                UiEvent::SessionResumed { workspace, .. } => {
                    workspace_path = workspace.clone();
                }
                _ => {}
            }
        }
    }
    let root = std::path::PathBuf::from(workspace_path);
    let loaded_at_start = oblivion_core::prompt::project_instructions(&root).map(|(name, _)| name);
    let agents_path = root.join("AGENTS.md");
    let exists = agents_path.is_file();
    let content = if exists {
        std::fs::read_to_string(&agents_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mems = st.store.list_memories(None, 50).unwrap_or_default();
    let proposed_mems: Vec<oblivion_core::extract::ProposedMemory> = mems
        .into_iter()
        .map(|m| oblivion_core::extract::ProposedMemory::new(m.mem_type, m.text, m.importance))
        .collect();
    let proposed_additions = oblivion_core::agents_md::propose_update(&root, &proposed_mems)
        .map(|p| p.additions)
        .unwrap_or_default();

    Ok(Json(AgentsMdResponse {
        file: "AGENTS.md".to_string(),
        exists,
        content,
        loaded_at_start,
        proposed_additions,
    }))
}

async fn apply_agents_md(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<AgentsMdApplyResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut workspace_path = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    {
        let h = st.bus.history.lock().unwrap();
        for env in h.iter() {
            match &env.event {
                UiEvent::State(s) => {
                    workspace_path = s.root.clone();
                }
                UiEvent::SessionResumed { workspace, .. } => {
                    workspace_path = workspace.clone();
                }
                _ => {}
            }
        }
    }
    let root = std::path::PathBuf::from(workspace_path);
    let mems = st.store.list_memories(None, 50).unwrap_or_default();
    let proposed_mems: Vec<oblivion_core::extract::ProposedMemory> = mems
        .into_iter()
        .map(|m| oblivion_core::extract::ProposedMemory::new(m.mem_type, m.text, m.importance))
        .collect();

    match oblivion_core::agents_md::propose_update(&root, &proposed_mems) {
        Some(proposal) => {
            let additions = proposal.additions.clone();
            let count = additions.len();
            match oblivion_core::agents_md::apply_update(&proposal) {
                Ok(()) => Ok(Json(AgentsMdApplyResponse {
                    status: "ok".to_string(),
                    file: "AGENTS.md".to_string(),
                    added_count: count,
                    additions,
                })),
                Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        None => Ok(Json(AgentsMdApplyResponse {
            status: "up_to_date".to_string(),
            file: "AGENTS.md".to_string(),
            added_count: 0,
            additions: Vec::new(),
        })),
    }
}

fn scan_recent_files(root: &std::path::Path, max_count: usize) -> Vec<DashboardFile> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return files;
    }
    let now = std::time::SystemTime::now();
    let seven_days = std::time::Duration::from_secs(7 * 24 * 3600);

    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 4 {
            continue;
        }
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.starts_with('.')
                || file_name == "target"
                || file_name == "node_modules"
                || file_name == "__pycache__"
                || file_name == ".venv"
                || file_name == ".pytest_cache"
                || file_name == ".git"
            {
                continue;
            }
            if path.is_dir() {
                stack.push((path, depth + 1));
            } else if path.is_file() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        let age = now.duration_since(mtime).unwrap_or_default();
                        if age <= seven_days {
                            let size = meta.len();
                            let modified_secs = mtime
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            let ext = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_lowercase();
                            let lower_name = file_name.to_lowercase();
                            let category = if ext == "pdf" {
                                "paper"
                            } else if matches!(
                                ext.as_str(),
                                "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico"
                            ) {
                                "picture"
                            } else if matches!(
                                ext.as_str(),
                                "doc" | "docx" | "txt" | "rtf" | "csv" | "tsv" | "json"
                            ) {
                                "document"
                            } else if ext == "md" {
                                if lower_name.contains("case") || lower_name.contains("study") {
                                    "case-study"
                                } else {
                                    "notebook"
                                }
                            } else if matches!(
                                ext.as_str(),
                                "rs" | "py"
                                    | "js"
                                    | "ts"
                                    | "html"
                                    | "css"
                                    | "toml"
                                    | "yaml"
                                    | "yml"
                            ) {
                                "code"
                            } else {
                                "file"
                            };

                            let rel_path = path
                                .strip_prefix(root)
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|_| file_name.clone());

                            files.push(DashboardFile {
                                name: file_name,
                                path: rel_path,
                                size,
                                modified: modified_secs,
                                category: category.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    files.sort_by_key(|a| std::cmp::Reverse(a.modified));
    if files.len() > max_count {
        files.truncate(max_count);
    }
    files
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
        let store = Arc::new(oblivion_store::Store::in_memory().unwrap());
        let state = WebState {
            token: token.clone().into(),
            controls: controls.clone(),
            bus: bus.clone(),
            store,
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

    #[tokio::test]
    async fn memory_api_endpoints_crud_and_export() {
        let (url, _seen) = fake_server(vec![(200, sse(&[]))]).await;
        let dir = tempfile::tempdir().unwrap();
        let (_w, controls, _events) = worker(&url, dir.path());

        let store = Arc::new(oblivion_store::Store::in_memory().unwrap());
        let token = new_token();
        let (listener, addr) = bind(0).await.unwrap();
        let bus = Arc::new(Bus::default());
        let state = WebState {
            token: token.clone().into(),
            controls,
            bus,
            store: store.clone(),
        };
        tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        let base = format!("http://{addr}");
        let client = reqwest::Client::new();

        // 1. Unauthorized request fails
        let unauth = client
            .get(format!("{base}/api/memory"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. Create memory
        let create_res = client
            .post(format!("{base}/api/memory"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({
                "type": "user_pref",
                "text": "User prefers dark mode",
                "importance": 5
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);
        let created: oblivion_store::Memory = create_res.json().await.unwrap();
        assert_eq!(created.mem_type, "user_pref");
        assert_eq!(created.text, "User prefers dark mode");
        assert_eq!(created.importance, 5);

        // 3. List memories
        let list_res = client
            .get(format!("{base}/api/memory"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(list_res.status(), StatusCode::OK);
        let list: Vec<oblivion_store::Memory> = list_res.json().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);

        // 4. Update memory
        let update_res = client
            .put(format!("{base}/api/memory/{}", created.id))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({
                "text": "User prefers high contrast dark mode",
                "importance": 4
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(update_res.status(), StatusCode::OK);
        let updated: oblivion_store::Memory = update_res.json().await.unwrap();
        assert_eq!(updated.text, "User prefers high contrast dark mode");
        assert_eq!(updated.importance, 4);

        // 5. Search memories
        let search_res = client
            .get(format!("{base}/api/memory?q=contrast"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(search_res.status(), StatusCode::OK);
        let search_list: Vec<oblivion_store::Memory> = search_res.json().await.unwrap();
        assert_eq!(search_list.len(), 1);

        // 6. Export memories (Markdown & JSON)
        let md_res = client
            .get(format!("{base}/api/memory/export?format=markdown"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(md_res.status(), StatusCode::OK);
        let md_text = md_res.text().await.unwrap();
        assert!(md_text.contains("User prefers high contrast dark mode"));

        // 7. Forget topic
        let forget_res = client
            .post(format!("{base}/api/memory/forget"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"query": "contrast"}))
            .send()
            .await
            .unwrap();
        assert_eq!(forget_res.status(), StatusCode::OK);

        // 8. Delete memory (should be 404 since forget deleted it)
        let del_res = client
            .delete(format!("{base}/api/memory/{}", created.id))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(del_res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn sessions_api_endpoints_crud_and_resume() {
        use oblivion_core::SessionStore;
        let dir = tempfile::tempdir().unwrap();
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let store = Arc::new(oblivion_store::Store::in_memory().unwrap());
        let token = new_token();
        let state = WebState {
            token: token.clone().into(),
            controls,
            bus,
            store: store.clone(),
        };

        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let base = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::new();

        // 1. Seed store with a session
        let mut s = oblivion_core::Session::new(dir.path().to_path_buf());
        s.id = "session-test-123".into();
        s.messages
            .push(oblivion_providers::Message::user("Fix auth bug"));
        store.save(&s).unwrap();

        // 2. List sessions
        let list_res = client
            .get(format!("{base}/api/sessions"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(list_res.status(), StatusCode::OK);
        let list: Vec<oblivion_core::SessionInfo> = list_res.json().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "session-test-123");
        assert_eq!(list[0].first_prompt, "Fix auth bug");

        // 3. Get single session
        let get_res = client
            .get(format!("{base}/api/sessions/session-test-123"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(get_res.status(), StatusCode::OK);
        let loaded: oblivion_core::Session = get_res.json().await.unwrap();
        assert_eq!(loaded.id, "session-test-123");

        // 4. Rename session
        let rename_res = client
            .post(format!("{base}/api/sessions/session-test-123/rename"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({ "title": "Refactor authentication flow" }))
            .send()
            .await
            .unwrap();
        assert_eq!(rename_res.status(), StatusCode::OK);
        let list_after_rename = store.list_sessions(10).unwrap();
        assert_eq!(
            list_after_rename[0].first_prompt,
            "Refactor authentication flow"
        );

        // 5. Update workspace
        let ws_res = client
            .post(format!("{base}/api/sessions/session-test-123/workspace"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({ "workspace": dir.path().to_string_lossy() }))
            .send()
            .await
            .unwrap();
        assert_eq!(ws_res.status(), StatusCode::OK);

        // 6. Resume session
        let resume_res = client
            .post(format!("{base}/api/sessions/session-test-123/resume"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(resume_res.status(), StatusCode::OK);

        // 7. Delete session
        let del_res = client
            .delete(format!("{base}/api/sessions/session-test-123"))
            .header("x-oblivion-token", &token)
            .send()
            .await
            .unwrap();
        assert_eq!(del_res.status(), StatusCode::OK);
        assert_eq!(store.list_sessions(10).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn web_dashboard_rest_api_returns_real_stats() {
        use oblivion_providers::UsageLedger;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("oblivion.db");
        let store = Arc::new(oblivion_store::Store::open(&db_path).unwrap());

        // Record usage in store
        store.record(
            "groq:llama-3.3-70b-versatile",
            Some(&oblivion_providers::Usage {
                prompt_tokens: 1500,
                completion_tokens: 350,
            }),
        );

        // Add a memory
        let _ = store.add_memory(&oblivion_store::NewMemory {
            mem_type: "user_pref".into(),
            text: "Prefers concise responses".into(),
            source_session: None,
            project: None,
            importance: 4,
        });

        // Create a test file in dir
        let test_file = dir.path().join("notes.md");
        std::fs::write(&test_file, "# Research Notes").unwrap();

        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        bus.publish(UiEvent::SessionResumed {
            id: "s1".into(),
            workspace: dir.path().to_string_lossy().into_owned(),
            messages: vec![],
        });
        bus.publish(UiEvent::Model {
            label: "groq:llama-3.3-70b-versatile".into(),
        });
        bus.publish(UiEvent::Todos {
            items: vec![crate::app::TodoView {
                text: "Refactor dashboard".into(),
                status: "in_progress".into(),
            }],
        });

        let token = "dashboard-token-1234";
        let state = WebState {
            token: token.into(),
            controls,
            bus,
            store: store.clone(),
        };

        let app = router(state);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{}", addr.port());

        // 1. Unauthorized request
        let unauth = client
            .get(format!("{base}/api/system/dashboard"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. Authorized request
        let res = client
            .get(format!("{base}/api/system/dashboard"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let data: DashboardData = res.json().await.unwrap();

        assert_eq!(data.usage_today.len(), 1);
        assert_eq!(data.usage_today[0].target, "groq:llama-3.3-70b-versatile");
        assert_eq!(data.usage_today[0].requests, 1);
        assert_eq!(data.usage_today[0].prompt_tokens, 1500);

        assert_eq!(data.memories_count, 1);
        assert_eq!(data.memories_by_type.get("user_pref"), Some(&1));

        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].text, "Refactor dashboard");
        assert_eq!(data.tasks[0].status, "in_progress");

        assert_eq!(data.engine.status, "ready");
        assert_eq!(data.engine.model, "groq:llama-3.3-70b-versatile");

        assert!(data.recent_files.iter().any(|f| f.name == "notes.md"));
    }

    #[tokio::test]
    async fn web_agents_md_endpoints_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-agents-md";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // Write initial AGENTS.md in workspace dir
        let ws_dir = dir.path().join("workspace");
        std::fs::create_dir_all(&ws_dir).unwrap();
        let initial_text = "# Project Rules\n- cargo test before PR\n";
        std::fs::write(ws_dir.join("AGENTS.md"), initial_text).unwrap();

        // Publish state with the workspace dir
        bus.publish(UiEvent::State(crate::app::StateView {
            version: "0.1.0".into(),
            root: ws_dir.to_string_lossy().into_owned(),
            session: "test-sess-1".into(),
            approval: "ask".into(),
            models: vec![],
            todos: vec![],
            skills: vec![],
            browser: false,
            instructions_file: Some("AGENTS.md".into()),
        }));

        // Add a project fact memory
        let mem = oblivion_store::NewMemory::new(
            "project_fact",
            "SQLite database is stored in store.db",
            5,
        );
        store.add_memory(&mem).unwrap();

        // 1. GET /api/system/agents-md
        let res = client
            .get(format!("{base}/api/system/agents-md"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let resp: AgentsMdResponse = res.json().await.unwrap();
        assert_eq!(resp.file, "AGENTS.md");
        assert!(resp.exists);
        assert!(resp.content.contains("cargo test before PR"));
        assert_eq!(resp.proposed_additions.len(), 1);
        assert_eq!(
            resp.proposed_additions[0],
            "SQLite database is stored in store.db"
        );

        // 2. POST /api/system/agents-md/apply
        let apply_res = client
            .post(format!("{base}/api/system/agents-md/apply"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(apply_res.status(), StatusCode::OK);
        let apply_resp: AgentsMdApplyResponse = apply_res.json().await.unwrap();
        assert_eq!(apply_resp.status, "ok");
        assert_eq!(apply_resp.added_count, 1);

        // Verify file on disk
        let disk_content = std::fs::read_to_string(ws_dir.join("AGENTS.md")).unwrap();
        assert!(disk_content.contains("cargo test before PR"));
        assert!(disk_content.contains("SQLite database is stored in store.db"));

        // 3. GET /api/system/agents-md now reports no pending additions
        let res2 = client
            .get(format!("{base}/api/system/agents-md"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        let resp2: AgentsMdResponse = res2.json().await.unwrap();
        assert_eq!(resp2.proposed_additions.len(), 0);
    }

    #[tokio::test]
    async fn web_hardware_endpoint_returns_specs_and_recommendation() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-hw";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. Unauthorized
        let unauth = client
            .get(format!("{base}/api/system/hardware"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. Authorized
        let res = client
            .get(format!("{base}/api/system/hardware"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let data: HardwareResponse = res.json().await.unwrap();
        assert!(data.hardware.total_ram_gb > 0.0);
        assert!(!data.hardware.gpu_name.is_empty());
        assert!(!data.recommendation.recommended_size.is_empty());
        assert!(!data.recommendation.recommended_models.is_empty());
        assert!(!data.recommendation.tradeoff_explanation.is_empty());
    }
}
