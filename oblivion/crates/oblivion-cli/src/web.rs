//! `oblivion --web`: the agent in a local web page.
//!
//! Safety: the server listens on 127.0.0.1 only, and every API call must carry a
//! random token (in the page URL), so other websites open in the same browser
//! cannot drive the agent. Events stream to the page with Server-Sent Events;
//! a page that reconnects first receives everything so far.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use futures_util::stream::{self, Stream, StreamExt};
use oblivion_core::Decision;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::app::{Controls, UiEvent};

const PAGE: &str = include_str!("../web/index.html");
const DESKTOP_PAGE: &str = include_str!("../../../../web/templates/index.html");

#[derive(rust_embed::RustEmbed)]
#[folder = "../../../web/static/"]
pub struct StaticAssets;
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
    pub ollama: Option<Arc<oblivion_core::OllamaClient>>,
    pub routing: Arc<std::sync::RwLock<oblivion_core::config::RoutingSettings>>,
    pub config_path: Option<PathBuf>,
    pub voice_transcriber: Option<Arc<dyn oblivion_core::VoiceTranscriber>>,
    pub voice_synthesizer: Option<Arc<dyn oblivion_core::VoiceSynthesizer>>,
    pub wake_word_enabled: Arc<std::sync::atomic::AtomicBool>,
}

impl WebState {
    pub fn config_file_path(&self) -> PathBuf {
        self.config_path
            .clone()
            .unwrap_or_else(oblivion_core::config::default_config_path)
    }

    pub fn ollama_client(&self) -> oblivion_core::OllamaClient {
        if let Some(c) = &self.ollama {
            (**c).clone()
        } else {
            oblivion_core::OllamaClient::new("http://localhost:11434")
        }
    }

    pub fn voice_service(&self) -> oblivion_core::VoiceService {
        let is_wake_word_on = self
            .wake_word_enabled
            .load(std::sync::atomic::Ordering::SeqCst);
        if let Some(t) = &self.voice_transcriber {
            let offline = self.routing.read().unwrap().offline;
            let svc = oblivion_core::VoiceService::new(
                t.clone(),
                None,
                self.voice_synthesizer.clone(),
                "auto",
                "Ctrl+Space",
                "en",
                offline,
            );
            svc.set_wake_word_enabled(is_wake_word_on);
            return svc;
        }

        let cfg = oblivion_core::Config::load(&self.config_file_path()).unwrap_or_default();
        let voice_cfg = cfg.voice;
        let offline = self.routing.read().unwrap().offline;

        let local = Arc::new(oblivion_core::LocalWhisperTranscriber::new(
            voice_cfg.whisper_path.map(PathBuf::from),
            None,
        ));

        let cloud: Option<Arc<dyn oblivion_core::VoiceTranscriber>> = if !offline {
            if let Ok(groq_key) = std::env::var("GROQ_API_KEY") {
                if !groq_key.trim().is_empty() {
                    Some(Arc::new(oblivion_core::CloudWhisperTranscriber::groq(
                        groq_key,
                    )))
                } else {
                    None
                }
            } else if let Ok(openai_key) = std::env::var("OPENAI_API_KEY") {
                if !openai_key.trim().is_empty() {
                    Some(Arc::new(oblivion_core::CloudWhisperTranscriber::openai(
                        openai_key,
                    )))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let synth = self.voice_synthesizer.clone().or_else(|| {
            Some(Arc::new(oblivion_core::LocalPiperSynthesizer::new(
                None, None,
            )))
        });

        let svc = oblivion_core::VoiceService::new(
            local,
            cloud,
            synth,
            voice_cfg.stt_engine,
            voice_cfg.hotkey,
            voice_cfg.language,
            offline,
        );
        svc.set_wake_word_enabled(is_wake_word_on);
        svc
    }

    fn authorized(&self, headers: &HeaderMap, query: &HashMap<String, String>) -> bool {
        let given = headers
            .get("x-oblivion-token")
            .or_else(|| headers.get("x-jarvis-token"))
            .and_then(|v| v.to_str().ok())
            .or(query.get("t").map(String::as_str))
            .unwrap_or("");
        constant_time_eq(given.as_bytes(), self.token.as_bytes())
    }
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

async fn static_file(Path(path): Path<String>) -> Response {
    let clean = path.replace('\\', "/");
    let clean = clean.trim_start_matches('/');
    match StaticAssets::get(clean) {
        Some(content) => {
            let mime = content_type(clean);
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "no-cache, must-revalidate"),
                ],
                content.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Static file not found").into_response(),
    }
}

#[derive(Serialize)]
struct StatusResponse {
    running: bool,
    port: u16,
    error: Option<String>,
}

async fn get_status(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(StatusResponse {
            running: true,
            port: 7878,
            error: None,
        }),
    )
        .into_response()
}

#[derive(Deserialize)]
struct ChatBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    images: Vec<oblivion_core::ImageAttachment>,
}

async fn chat_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<ChatBody>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let msg = if !body.message.is_empty() {
        body.message
    } else {
        body.text
    };
    if msg.trim().is_empty() && body.images.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "empty message"})),
        )
            .into_response();
    }
    let msg = if msg.trim().is_empty() && !body.images.is_empty() {
        "Describe this image.".to_string()
    } else {
        msg
    };
    if !body.images.is_empty() {
        st.controls.send_with_images(msg, body.images);
    } else {
        st.controls.send(msg);
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({"response": "Message sent to agent"})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct SetAutostartBody {
    enabled: bool,
}

async fn get_autostart_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    match crate::autostart::get_status() {
        Ok(status) => (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn set_autostart_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<SetAutostartBody>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    match crate::autostart::set_enabled(body.enabled) {
        Ok(status) => (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_wizard_status_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let p = st.config_file_path();
    let status = crate::wizard::get_wizard_status(&p);
    (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response()
}

async fn complete_wizard_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(req): Json<crate::wizard::WizardCompleteRequest>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let p = st.config_file_path();
    if let Ok(mut r) = st.routing.write() {
        r.offline = req.offline;
    }
    match crate::wizard::complete_wizard(&p, &req) {
        Ok(status) => (StatusCode::OK, Json(serde_json::to_value(status).unwrap())).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn test_wizard_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !st.authorized(&headers, &q) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "unauthorized"})),
        )
            .into_response();
    }
    let p = st.config_file_path();
    let status = crate::wizard::get_wizard_status(&p);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "message": "Connection and environment verified",
            "offline": status.offline,
            "workspace": status.workspace
        })),
    )
        .into_response()
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
        // Page and static assets
        .route("/", get(page))
        .route("/classic", get(classic_page))
        .route("/static/{*path}", get(static_file))
        // Live stream and controls
        .route("/events", get(events))
        .route("/api/oblivion/events", get(events))
        .route("/send", post(send))
        .route("/api/oblivion/send", post(send))
        .route("/api/oblivion/input", post(send))
        .route("/approve", post(approve))
        .route("/api/oblivion/approve", post(approve))
        .route("/cancel", post(cancel))
        .route("/api/oblivion/cancel", post(cancel))
        .route("/api/oblivion/start", post(get_status).get(get_status))
        .route("/api/oblivion/status", get(get_status))
        .route("/api/chat", post(chat_handler))
        .route("/api/screen/capture", post(screen_capture))
        .route("/api/oblivion/screen/capture", post(screen_capture))
        // System & dashboard
        .route("/api/system/dashboard", get(get_dashboard))
        .route("/api/oblivion/dashboard", get(get_dashboard))
        .route("/api/system/hardware", get(get_hardware))
        .route("/api/oblivion/system/hardware", get(get_hardware))
        .route(
            "/api/system/agents-md",
            get(get_agents_md).post(apply_agents_md),
        )
        .route(
            "/api/oblivion/system/agents-md",
            get(get_agents_md).post(apply_agents_md),
        )
        .route("/api/system/agents-md/apply", post(apply_agents_md))
        .route(
            "/api/oblivion/system/agents-md/apply",
            post(apply_agents_md),
        )
        // Autostart
        .route(
            "/api/system/autostart",
            get(get_autostart_handler).post(set_autostart_handler),
        )
        .route(
            "/api/oblivion/system/autostart",
            get(get_autostart_handler).post(set_autostart_handler),
        )
        // First-run Wizard
        .route("/api/system/wizard/status", get(get_wizard_status_handler))
        .route(
            "/api/oblivion/system/wizard/status",
            get(get_wizard_status_handler),
        )
        .route("/api/system/wizard/complete", post(complete_wizard_handler))
        .route(
            "/api/oblivion/system/wizard/complete",
            post(complete_wizard_handler),
        )
        .route("/api/system/wizard/test", post(test_wizard_handler))
        .route(
            "/api/oblivion/system/wizard/test",
            post(test_wizard_handler),
        )
        // Models & routing
        .route("/api/models", get(get_models))
        .route("/api/oblivion/models", get(get_models))
        .route("/api/models/pull", post(pull_model))
        .route("/api/oblivion/models/pull", post(pull_model))
        .route("/api/models/delete", post(delete_model_handler))
        .route("/api/oblivion/models/delete", post(delete_model_handler))
        .route("/api/models/speed-test", post(speed_test_handler))
        .route("/api/oblivion/models/speed-test", post(speed_test_handler))
        .route("/api/routing", get(get_routing).post(update_routing))
        .route(
            "/api/oblivion/routing",
            get(get_routing).post(update_routing),
        )
        // Memory
        .route(
            "/api/memory",
            get(list_or_search_memories).post(create_memory),
        )
        .route(
            "/api/oblivion/memory",
            get(list_or_search_memories).post(create_memory),
        )
        .route("/api/memory/{id}", put(update_memory).delete(delete_memory))
        .route(
            "/api/oblivion/memory/{id}",
            put(update_memory).delete(delete_memory),
        )
        .route("/api/memory/forget", post(forget_memories_topic))
        .route("/api/oblivion/memory/forget", post(forget_memories_topic))
        .route("/api/memory/export", get(export_memories))
        .route("/api/oblivion/memory/export", get(export_memories))
        // Sessions
        .route(
            "/api/sessions",
            get(list_or_search_sessions).post(create_or_switch_session),
        )
        .route(
            "/api/oblivion/sessions",
            get(list_or_search_sessions).post(create_or_switch_session),
        )
        .route(
            "/api/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route(
            "/api/oblivion/sessions/{id}",
            get(get_session).delete(delete_session),
        )
        .route("/api/sessions/{id}/rename", post(rename_session))
        .route("/api/oblivion/sessions/{id}/rename", post(rename_session))
        .route("/api/sessions/{id}/resume", post(resume_session))
        .route("/api/oblivion/sessions/{id}/resume", post(resume_session))
        .route(
            "/api/sessions/{id}/workspace",
            post(update_session_workspace),
        )
        .route(
            "/api/oblivion/sessions/{id}/workspace",
            post(update_session_workspace),
        )
        // Voice & STT
        .route("/api/voice/status", get(get_voice_status))
        .route("/api/oblivion/voice/status", get(get_voice_status))
        .route("/api/voice/transcribe", post(post_voice_transcribe))
        .route(
            "/api/oblivion/voice/transcribe",
            post(post_voice_transcribe),
        )
        // Voice TTS
        .route("/api/voice/tts/speak", post(post_voice_tts_speak))
        .route("/api/oblivion/voice/tts/speak", post(post_voice_tts_speak))
        // Voice Wake Word
        .route(
            "/api/voice/wake-word",
            get(get_wake_word_status).post(post_wake_word_toggle),
        )
        .route(
            "/api/oblivion/voice/wake-word",
            get(get_wake_word_status).post(post_wake_word_toggle),
        )
        // Computer Use
        .route(
            "/api/computer/status",
            get(get_computer_status).post(post_computer_status),
        )
        .route(
            "/api/oblivion/computer/status",
            get(get_computer_status).post(post_computer_status),
        )
        .route("/api/computer/stop", post(post_computer_stop))
        .route("/api/oblivion/computer/stop", post(post_computer_stop))
        .route("/api/computer/reset", post(post_computer_reset))
        .route("/api/oblivion/computer/reset", post(post_computer_reset))
        .with_state(state)
}

async fn page(State(st): State<WebState>) -> Response {
    let html = DESKTOP_PAGE.replace("{{ jarvis_token }}", &st.token);
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data: https:; connect-src 'self' https:; frame-ancestors 'none'",
            ),
        ],
        Html(html),
    )
        .into_response()
}

async fn classic_page() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'",
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
    #[serde(default)]
    text: String,
    #[serde(default)]
    voice: bool,
    #[serde(default)]
    images: Vec<oblivion_core::ImageAttachment>,
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
    if body.text.trim().is_empty() && body.images.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    let text = if body.text.trim().is_empty() && !body.images.is_empty() {
        "Describe this image.".to_string()
    } else {
        body.text
    };
    if body.voice {
        st.controls.send_voice(text);
    } else if !body.images.is_empty() {
        st.controls.send_with_images(text, body.images);
    } else {
        st.controls.send(text);
    }
    StatusCode::NO_CONTENT
}

async fn screen_capture(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match crate::screen::capture_screen() {
        Ok(att) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "image": att,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": false,
                "error": e.to_string(),
                "needs_client_capture": true,
            })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct ApproveBody {
    id: u64,
    answer: String,
    #[serde(default)]
    source: Option<String>,
}

async fn approve(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<ApproveBody>,
) -> Response {
    if !st.authorized(&headers, &q) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let decision = match body.answer.as_str() {
        "yes" => Decision::Yes,
        "always" => Decision::Always,
        "no" => Decision::No,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    match st
        .controls
        .approvals
        .answer_with_source(body.id, decision, body.source.as_deref())
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(crate::app::ApprovalError::VoiceNotAllowed(msg)) => (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": msg,
                "code": "VOICE_APPROVAL_DISALLOWED"
            })),
        )
            .into_response(),
        Err(crate::app::ApprovalError::NotFound) => StatusCode::GONE.into_response(),
        Err(crate::app::ApprovalError::ChannelClosed) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
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
pub struct ModelsResponse {
    pub status: oblivion_core::OllamaStatus,
    pub models: Vec<oblivion_core::OllamaModel>,
    pub recommendation: oblivion_core::ModelRecommendation,
}

#[derive(Deserialize)]
struct PullModelRequest {
    name: String,
}

#[derive(Deserialize)]
struct DeleteModelRequest {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteModelResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Deserialize)]
struct SpeedTestRequest {
    model: String,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedTestResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<oblivion_core::OllamaSpeedTestResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

async fn get_models(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<ModelsResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let client = st.ollama_client();
    let status = client.check_status().await;
    let models = if status.running {
        client.list_models().await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let hardware = oblivion_core::detect_hardware();
    let recommendation = oblivion_core::recommend_model(&hardware);

    Ok(Json(ModelsResponse {
        status,
        models,
        recommendation,
    }))
}

async fn pull_model(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<PullModelRequest>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let model_name = body.name.trim();
    if model_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = st.ollama_client();
    let stream_res = client.pull_model_stream(model_name).await;
    let event_stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        match stream_res {
            Ok(s) => Box::pin(s.map(|item| {
                let json_str = match item {
                    Ok(progress) => serde_json::to_string(&progress).unwrap_or_default(),
                    Err(e) => serde_json::json!({
                        "status": "error",
                        "error": e.to_string()
                    })
                    .to_string(),
                };
                Ok(Event::default().data(json_str))
            })),
            Err(e) => {
                let err_ev = serde_json::json!({
                    "status": "error",
                    "error": e.to_string()
                });
                Box::pin(stream::once(async move {
                    Ok(Event::default().data(err_ev.to_string()))
                }))
            }
        };

    Ok(Sse::new(event_stream).keep_alive(KeepAlive::default()))
}

async fn delete_model_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<DeleteModelRequest>,
) -> Result<Json<DeleteModelResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let model_name = body.name.trim();
    if model_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = st.ollama_client();
    match client.delete_model(model_name).await {
        Ok(()) => Ok(Json(DeleteModelResponse {
            status: "ok".into(),
            message: None,
        })),
        Err(e) => Ok(Json(DeleteModelResponse {
            status: "error".into(),
            message: Some(e.to_string()),
        })),
    }
}

async fn speed_test_handler(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<SpeedTestRequest>,
) -> Result<Json<SpeedTestResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let model_name = body.model.trim();
    if model_name.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = st.ollama_client();
    match client.speed_test(model_name, body.prompt.as_deref()).await {
        Ok(res) => Ok(Json(SpeedTestResponse {
            status: "ok".into(),
            result: Some(res),
            error: None,
        })),
        Err(e) => Ok(Json(SpeedTestResponse {
            status: "error".into(),
            result: None,
            error: Some(e.to_string()),
        })),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingResponse {
    pub offline: bool,
    pub rules: oblivion_core::routing::RoutingRules,
    pub classes: Vec<RequestClassInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestClassInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub priority: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateRoutingRequest {
    #[serde(default)]
    pub offline: Option<bool>,
    #[serde(default)]
    pub rules: Option<oblivion_core::routing::RoutingRules>,
}

async fn get_routing(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<RoutingResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let r = st.routing.read().unwrap();
    let rules = r.rules.clone();
    let offline = r.offline;
    let classes = vec![
        RequestClassInfo {
            id: "private".into(),
            label: "Private / Sensitive".into(),
            description: "Passwords, tokens, credentials, and offline mode. Cloud models are forbidden; routes strictly to local models.".into(),
            priority: rules.private.clone(),
        },
        RequestClassInfo {
            id: "simple".into(),
            label: "Simple".into(),
            description: "Quick questions, fast edits, basic explanations.".into(),
            priority: rules.simple.clone(),
        },
        RequestClassInfo {
            id: "hard".into(),
            label: "Hard / Complex".into(),
            description: "Complex refactoring, architectural redesign, debugging, security audits.".into(),
            priority: rules.hard.clone(),
        },
        RequestClassInfo {
            id: "vision".into(),
            label: "Vision".into(),
            description: "Requests inspecting screenshots, images, or pictures.".into(),
            priority: rules.vision.clone(),
        },
        RequestClassInfo {
            id: "long_context".into(),
            label: "Long Context".into(),
            description: "Conversation history + workspace context exceeds standard limits (> 16K tokens).".into(),
            priority: rules.long_context.clone(),
        },
    ];

    Ok(Json(RoutingResponse {
        offline,
        rules,
        classes,
    }))
}

async fn update_routing(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<UpdateRoutingRequest>,
) -> Result<Json<RoutingResponse>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    {
        let mut r = st.routing.write().unwrap();
        if let Some(off) = body.offline {
            r.offline = off;
        }
        if let Some(rules) = body.rules {
            r.rules = rules;
        }
    }
    get_routing(State(st), headers, Query(q)).await
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

#[derive(Debug, Deserialize)]
pub struct VoiceTranscribeRequest {
    #[serde(default)]
    pub audio_base64: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub format: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoiceTranscribeResponse {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: u64,
    pub engine: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoiceErrorResponse {
    pub error: String,
}

async fn get_voice_status(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<oblivion_core::VoiceStatus>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let service = st.voice_service();
    Ok(Json(service.status()))
}

async fn post_voice_transcribe(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    body: axum::body::Bytes,
) -> Result<Json<VoiceTranscribeResponse>, (StatusCode, Json<VoiceErrorResponse>)> {
    if !st.authorized(&headers, &q) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(VoiceErrorResponse {
                error: "unauthorized: invalid or missing token".into(),
            }),
        ));
    }

    if body.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(VoiceErrorResponse {
                error: "Empty audio payload".into(),
            }),
        ));
    }

    let (wav_bytes, requested_language) = if body.first() == Some(&b'{') {
        let req: VoiceTranscribeRequest = serde_json::from_slice(&body).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(VoiceErrorResponse {
                    error: format!("Invalid JSON request: {}", e),
                }),
            )
        })?;

        let decoded = if let Some(b64) = req.audio_base64 {
            base64::engine::general_purpose::STANDARD
                .decode(b64.trim())
                .map_err(|e| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(VoiceErrorResponse {
                            error: format!("Invalid base64 audio data: {}", e),
                        }),
                    )
                })?
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(VoiceErrorResponse {
                    error: "Missing 'audio_base64' field in JSON request".into(),
                }),
            ));
        };
        (decoded, req.language)
    } else {
        (body.to_vec(), None)
    };

    let audio = oblivion_core::AudioBuffer::from_wav_bytes(&wav_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(VoiceErrorResponse {
                error: format!("Failed to parse WAV audio buffer: {}", e),
            }),
        )
    })?;

    let service = st.voice_service();
    let result = service.transcribe(&audio).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(VoiceErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    Ok(Json(VoiceTranscribeResponse {
        text: result.text,
        language: requested_language.or(result.language),
        duration_ms: result.duration_ms,
        engine: result.engine,
    }))
}

#[derive(Debug, Deserialize)]
pub struct VoiceSpeakRequest {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VoiceSpeakResponse {
    pub text: String,
    pub duration_ms: u64,
    pub audio_base64: String,
    pub format: String,
}

async fn post_voice_tts_speak(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(req): Json<VoiceSpeakRequest>,
) -> Result<Json<VoiceSpeakResponse>, (StatusCode, Json<VoiceErrorResponse>)> {
    if !st.authorized(&headers, &q) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(VoiceErrorResponse {
                error: "unauthorized: invalid or missing token".into(),
            }),
        ));
    }

    if req.text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(VoiceErrorResponse {
                error: "Empty text to synthesize".into(),
            }),
        ));
    }

    let service = st.voice_service();
    let audio = service.synthesize(&req.text).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(VoiceErrorResponse {
                error: e.to_string(),
            }),
        )
    })?;

    let wav_bytes = audio.to_wav_bytes();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);

    Ok(Json(VoiceSpeakResponse {
        text: req.text,
        duration_ms: audio.duration_ms(),
        audio_base64: b64,
        format: "audio/wav".to_string(),
    }))
}

#[derive(Deserialize)]
struct WakeWordToggleBody {
    #[serde(default)]
    enabled: Option<bool>,
}

async fn get_wake_word_status(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let enabled = st
        .wake_word_enabled
        .load(std::sync::atomic::Ordering::SeqCst);
    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "phrases": ["jarvis", "hey jarvis"],
        "hotkey": "Ctrl+Space"
    })))
}

async fn post_wake_word_toggle(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<WakeWordToggleBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let new_state = match body.enabled {
        Some(val) => val,
        None => !st
            .wake_word_enabled
            .load(std::sync::atomic::Ordering::SeqCst),
    };
    st.wake_word_enabled
        .store(new_state, std::sync::atomic::Ordering::SeqCst);
    Ok(Json(serde_json::json!({
        "enabled": new_state,
        "phrases": ["jarvis", "hey jarvis"],
        "message": if new_state {
            "Wake word detection enabled (hands-free 'Hey Jarvis' keyword spotting)."
        } else {
            "Wake word detection disabled (push-to-talk only)."
        }
    })))
}

#[derive(Deserialize)]
struct ComputerSettingsUpdateBody {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    allowed_apps: Option<Vec<String>>,
}

async fn get_computer_status(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let cfg = oblivion_core::Config::load(&st.config_file_path()).unwrap_or_default();
    let enabled = cfg.computer_use.enabled || std::env::var("OBLIVION_ENABLE_COMPUTER_USE").is_ok();
    let stopped = oblivion_tools::is_emergency_stopped();
    let active_win = oblivion_tools::computer::get_active_window().ok();

    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "stopped": stopped,
        "allowed_apps": cfg.computer_use.allowed_apps,
        "stop_hotkey": cfg.computer_use.stop_hotkey,
        "ask_all": cfg.computer_use.ask_all,
        "active_window": active_win,
    })))
}

async fn post_computer_status(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
    Json(body): Json<ComputerSettingsUpdateBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let p = st.config_file_path();
    let base_text = if p.exists() {
        std::fs::read_to_string(&p)
            .unwrap_or_else(|_| oblivion_core::config::DEFAULT_CONFIG.to_string())
    } else {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        oblivion_core::config::DEFAULT_CONFIG.to_string()
    };

    let updated = oblivion_core::config::apply_computer_use_to_config_text(
        &base_text,
        body.enabled,
        body.allowed_apps.as_deref(),
    );
    let _ = std::fs::write(&p, updated);

    let cfg = oblivion_core::Config::load(&p).unwrap_or_default();
    let enabled = cfg.computer_use.enabled || std::env::var("OBLIVION_ENABLE_COMPUTER_USE").is_ok();
    let stopped = oblivion_tools::is_emergency_stopped();
    let active_win = oblivion_tools::computer::get_active_window().ok();

    Ok(Json(serde_json::json!({
        "enabled": enabled,
        "stopped": stopped,
        "allowed_apps": cfg.computer_use.allowed_apps,
        "stop_hotkey": cfg.computer_use.stop_hotkey,
        "ask_all": cfg.computer_use.ask_all,
        "active_window": active_win,
    })))
}

async fn post_computer_stop(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    oblivion_tools::trigger_emergency_stop();
    Ok(Json(serde_json::json!({
        "ok": true,
        "stopped": true,
        "message": "Emergency stop activated: all desktop computer operations halted."
    })))
}

async fn post_computer_reset(
    State(st): State<WebState>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !st.authorized(&headers, &q) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    oblivion_tools::reset_emergency_stop();
    Ok(Json(serde_json::json!({
        "ok": true,
        "stopped": false,
        "message": "Emergency stop reset: desktop computer operations resumed."
    })))
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
        bus.publish(UiEvent::User {
            text: "hi".into(),
            images: vec![],
        });
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        let page_text = page.text().await.unwrap();
        assert!(page_text.contains("<title>Jarvis - Executive Desktop</title>"));

        let classic_page = http.get(format!("{base}/classic")).send().await.unwrap();
        assert!(classic_page
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
        assert_eq!(ask["low_risk"], false);
        assert!(
            !dir.path().join("hello.txt").exists(),
            "nothing written before approval"
        );

        let id = ask["id"].as_u64().unwrap();

        // 1. Attempt voice approval on high-risk write_file: must be rejected with 403 Forbidden
        let voice_attempt = http
            .post(format!("{base}/approve"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"id": id, "answer": "yes", "source": "voice"}))
            .send()
            .await
            .unwrap();
        assert_eq!(voice_attempt.status(), 403);
        let err_json: Value = voice_attempt.json().await.unwrap();
        assert_eq!(err_json["code"], "VOICE_APPROVAL_DISALLOWED");
        assert!(
            !dir.path().join("hello.txt").exists(),
            "file must not be written after rejected voice approval"
        );

        // 2. Manual click approval succeeds
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn voice_approval_allowed_for_low_risk_actions() {
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), "external confidential data").unwrap();
        let outside_path = outside.path().to_str().unwrap().replace('\\', "/");

        let read = tool_call(
            "r1",
            "read_file",
            &format!(r#"{{"path":"{}"}}"#, outside_path),
        );
        let (url, _seen) = fake_server(vec![
            (200, sse(&[&read])),
            (200, sse(&[&text("Read outside file content.")])),
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        tokio::spawn(pump(events, bus));
        tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
        tokio::spawn(w.run());
        let base = format!("http://{addr}");
        let http = reqwest::Client::new();

        let es = http
            .get(format!("{base}/events?t={token}"))
            .send()
            .await
            .unwrap();
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
        let _st = next_of(&mut erx, "state").await;

        let r = http
            .post(format!("{base}/send"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"text":"read outside doc", "voice": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 204);

        let ask = next_of(&mut erx, "approval").await;
        assert_eq!(ask["tool"], "read_file");
        assert_eq!(
            ask["low_risk"], true,
            "read_file outside root must be marked low_risk"
        );
        let id = ask["id"].as_u64().unwrap();

        // Spoken / voice approval succeeds for low-risk action!
        let r_voice = http
            .post(format!("{base}/approve"))
            .header("x-oblivion-token", &token)
            .json(&serde_json::json!({"id": id, "answer": "yes", "source": "voice"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r_voice.status(), 204);

        let done_ev = next_of(&mut erx, "approval_done").await;
        assert_eq!(done_ev["answer"], "yes");

        let turn = next_of(&mut erx, "turn_end").await;
        assert_eq!(turn["outcome"], "done");
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            spoken_reply: false,
            wake_word_enabled: false,
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
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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

    #[tokio::test]
    async fn web_models_endpoints_work() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // 1. Mock Ollama HTTP server
        let ollama_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let ollama_addr = ollama_listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match ollama_listener.accept().await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 4096];
                    loop {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let req = String::from_utf8_lossy(&buf);
                    let (status, ctype, body) = if req.contains("GET /api/version") {
                        (
                            200,
                            "application/json",
                            r#"{"version":"0.5.4"}"#.to_string(),
                        )
                    } else if req.contains("GET /api/tags") {
                        (200, "application/json", r#"{"models":[{"name":"qwen2.5-coder:7b","size":4683074360,"digest":"abc","details":{"parameter_size":"7.6B","quantization_level":"Q4_K_M"}}]}"#.to_string())
                    } else if req.contains("POST /api/pull") {
                        (
                            200,
                            "application/x-ndjson",
                            "{\"status\":\"pulling manifest\"}\n{\"status\":\"success\"}\n"
                                .to_string(),
                        )
                    } else if req.contains("DELETE /api/delete") {
                        (
                            200,
                            "application/json",
                            r#"{"status":"success"}"#.to_string(),
                        )
                    } else if req.contains("POST /api/generate") {
                        (200, "application/json", r#"{"model":"qwen2.5-coder:7b","response":"gravity","eval_count":10,"eval_duration":500000000,"prompt_eval_count":5,"prompt_eval_duration":100000000,"load_duration":200000000,"total_duration":800000000}"#.to_string())
                    } else {
                        (404, "text/plain", "Not Found".to_string())
                    };

                    let resp = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let ollama_client = Arc::new(oblivion_core::OllamaClient::new(&format!(
            "http://{ollama_addr}"
        )));

        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-models";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
            ollama: Some(ollama_client),
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. GET /api/models unauth
        let unauth = client
            .get(format!("{base}/api/models"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. GET /api/models auth
        let res = client
            .get(format!("{base}/api/models"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let data: ModelsResponse = res.json().await.unwrap();
        assert!(data.status.running);
        assert_eq!(data.status.version.as_deref(), Some("0.5.4"));
        assert_eq!(data.models.len(), 1);
        assert_eq!(data.models[0].name, "qwen2.5-coder:7b");
        assert_eq!(data.models[0].size_formatted, "4.36 GB");
        assert!(!data.recommendation.recommended_size.is_empty());

        // 3. POST /api/models/pull auth
        let pull_res = client
            .post(format!("{base}/api/models/pull"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "name": "qwen2.5-coder:7b" }))
            .send()
            .await
            .unwrap();
        assert_eq!(pull_res.status(), StatusCode::OK);
        let pull_text = pull_res.text().await.unwrap();
        assert!(pull_text.contains("pulling manifest"));
        assert!(pull_text.contains("success"));

        // 4. POST /api/models/delete auth
        let del_res = client
            .post(format!("{base}/api/models/delete"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "name": "qwen2.5-coder:7b" }))
            .send()
            .await
            .unwrap();
        assert_eq!(del_res.status(), StatusCode::OK);
        let del_data: DeleteModelResponse = del_res.json().await.unwrap();
        assert_eq!(del_data.status, "ok");

        // 5. POST /api/models/speed-test auth
        let speed_res = client
            .post(format!("{base}/api/models/speed-test"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "model": "qwen2.5-coder:7b" }))
            .send()
            .await
            .unwrap();
        assert_eq!(speed_res.status(), StatusCode::OK);
        let speed_data: SpeedTestResponse = speed_res.json().await.unwrap();
        assert_eq!(speed_data.status, "ok");
        let result = speed_data.result.unwrap();
        assert_eq!(result.model, "qwen2.5-coder:7b");
        assert_eq!(result.eval_count, 10);
        assert!((result.tokens_per_second - 20.0).abs() < 0.1);
        assert_eq!(result.sample_text, "gravity");
    }

    #[tokio::test]
    async fn web_routing_endpoints_work() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-routing";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. GET /api/routing without auth -> 401
        let unauth = client
            .get(format!("{base}/api/routing"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. GET /api/routing with auth -> 200 OK
        let res = client
            .get(format!("{base}/api/routing"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let data: RoutingResponse = res.json().await.unwrap();
        assert!(!data.offline);
        assert!(!data.rules.private.is_empty());
        assert!(!data.rules.simple.is_empty());
        assert!(!data.rules.hard.is_empty());
        assert!(!data.rules.vision.is_empty());
        assert!(!data.rules.long_context.is_empty());
        assert_eq!(data.classes.len(), 5);

        // 3. POST /api/routing without auth -> 401
        let unauth_post = client
            .post(format!("{base}/api/routing"))
            .json(&serde_json::json!({ "offline": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth_post.status(), StatusCode::UNAUTHORIZED);

        // 4. POST /api/routing toggle offline -> true
        let post_res = client
            .post(format!("{base}/api/routing"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "offline": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(post_res.status(), StatusCode::OK);
        let updated: RoutingResponse = post_res.json().await.unwrap();
        assert!(updated.offline);

        // 5. GET confirms offline is true
        let res2 = client
            .get(format!("{base}/api/routing"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res2.status(), StatusCode::OK);
        let data2: RoutingResponse = res2.json().await.unwrap();
        assert!(data2.offline);

        // 6. POST /api/routing toggle offline -> false
        let post_res2 = client
            .post(format!("{base}/api/routing"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "offline": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(post_res2.status(), StatusCode::OK);
        let updated2: RoutingResponse = post_res2.json().await.unwrap();
        assert!(!updated2.offline);
    }

    #[tokio::test]
    async fn web_serves_desktop_and_embedded_static_assets() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-embedded-desktop";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. Root desktop page GET /
        let res = client.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
        let html = res.text().await.unwrap();
        assert!(html.contains("<title>Jarvis - Executive Desktop</title>"));
        // Check jarvis token replacement
        assert!(html.contains(token));
        assert!(!html.contains("{{ jarvis_token }}"));

        // 2. Classic Oblivion page GET /classic
        let res_classic = client.get(format!("{base}/classic")).send().await.unwrap();
        assert_eq!(res_classic.status(), StatusCode::OK);
        let classic_html = res_classic.text().await.unwrap();
        assert!(classic_html.contains("<title>Oblivion</title>"));

        // 3. Embedded static assets: CSS
        let res_css = client
            .get(format!("{base}/static/css/style.css"))
            .send()
            .await
            .unwrap();
        assert_eq!(res_css.status(), StatusCode::OK);
        assert_eq!(
            res_css.headers().get("content-type").unwrap(),
            "text/css; charset=utf-8"
        );
        let css = res_css.text().await.unwrap();
        assert!(css.contains("--bg-desktop"));

        // 4. Embedded static assets: JS
        let res_js = client
            .get(format!("{base}/static/js/app.js"))
            .send()
            .await
            .unwrap();
        assert_eq!(res_js.status(), StatusCode::OK);
        assert_eq!(
            res_js.headers().get("content-type").unwrap(),
            "application/javascript; charset=utf-8"
        );
        let js = res_js.text().await.unwrap();
        assert!(js.contains("document.addEventListener"));

        // 5. Embedded fonts
        let res_font = client
            .get(format!("{base}/static/fonts/JetBrainsMono-400-normal.ttf"))
            .send()
            .await
            .unwrap();
        assert_eq!(res_font.status(), StatusCode::OK);
        assert_eq!(res_font.headers().get("content-type").unwrap(), "font/ttf");
        let bytes = res_font.bytes().await.unwrap();
        assert!(!bytes.is_empty());

        // 6. Nonexistent static asset -> 404
        let res_404 = client
            .get(format!("{base}/static/nonexistent-file.xyz"))
            .send()
            .await
            .unwrap();
        assert_eq!(res_404.status(), StatusCode::NOT_FOUND);

        // 7. Desktop status endpoint with x-jarvis-token
        let status_res = client
            .get(format!("{base}/api/oblivion/status"))
            .header("x-jarvis-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(status_res.status(), StatusCode::OK);
        let status_json: serde_json::Value = status_res.json().await.unwrap();
        assert_eq!(status_json["running"], true);

        // 8. Desktop chat endpoint with x-jarvis-token
        let chat_res = client
            .post(format!("{base}/api/chat"))
            .header("x-jarvis-token", token)
            .json(&serde_json::json!({ "message": "hello from desktop test" }))
            .send()
            .await
            .unwrap();
        assert_eq!(chat_res.status(), StatusCode::OK);
        let chat_json: serde_json::Value = chat_res.json().await.unwrap();
        assert_eq!(chat_json["response"], "Message sent to agent");
    }

    #[tokio::test]
    async fn web_autostart_and_wizard_endpoints_work() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("oblivion.toml");
        let store = Arc::new(oblivion_store::Store::open(&dir.path().join("test.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-wizard";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: Some(cfg_path.clone()),
            voice_transcriber: None,
            voice_synthesizer: None,
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. Autostart endpoint: unauth -> 401
        let unauth = client
            .get(format!("{base}/api/system/autostart"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

        // 2. Autostart endpoint: auth -> 200 OK
        let autostart_res = client
            .get(format!("{base}/api/system/autostart"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(autostart_res.status(), StatusCode::OK);

        // 3. Wizard status endpoint: unauth -> 401
        let wizard_unauth = client
            .get(format!("{base}/api/system/wizard/status"))
            .send()
            .await
            .unwrap();
        assert_eq!(wizard_unauth.status(), StatusCode::UNAUTHORIZED);

        // 4. Wizard status endpoint: auth -> completed: false
        let wiz_status = client
            .get(format!("{base}/api/system/wizard/status"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(wiz_status.status(), StatusCode::OK);
        let s0: crate::wizard::WizardStatus = wiz_status.json().await.unwrap();
        assert!(!s0.completed);
        assert!(!s0.offline);

        // 5. Complete wizard via POST /api/system/wizard/complete
        let custom_ws = dir.path().join("my_new_workspace");
        let complete_res = client
            .post(format!("{base}/api/system/wizard/complete"))
            .header("x-oblivion-token", token)
            .json(&crate::wizard::WizardCompleteRequest {
                workspace: Some(custom_ws.to_string_lossy().to_string()),
                offline: true,
                autostart: false,
                api_keys: std::collections::HashMap::new(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(complete_res.status(), StatusCode::OK);
        let s1: crate::wizard::WizardStatus = complete_res.json().await.unwrap();
        assert!(s1.completed);
        assert!(s1.offline);
        assert_eq!(std::path::Path::new(&s1.workspace), custom_ws.as_path());
        assert!(custom_ws.exists());

        // 6. Test wizard test connection endpoint
        let test_res = client
            .post(format!("{base}/api/system/wizard/test"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(test_res.status(), StatusCode::OK);
        let test_json: serde_json::Value = test_res.json().await.unwrap();
        assert_eq!(test_json["status"], "ok");

        // 7. Re-check wizard status via alias route /api/oblivion/system/wizard/status
        let alias_res = client
            .get(format!("{base}/api/oblivion/system/wizard/status"))
            .header("x-jarvis-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(alias_res.status(), StatusCode::OK);
        let s2: crate::wizard::WizardStatus = alias_res.json().await.unwrap();
        assert!(s2.completed);
        assert!(s2.offline);
    }

    fn make_test_wav(sample_rate: u32, duration_ms: u64) -> Vec<u8> {
        let num_samples = ((sample_rate as u64 * duration_ms) / 1000) as usize;
        let samples = vec![1000i16; num_samples];
        let audio = oblivion_core::AudioBuffer::new_pcm16_mono(sample_rate, &samples);
        audio.to_wav_bytes()
    }

    #[tokio::test]
    async fn test_voice_status_and_transcribe_api() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            Arc::new(oblivion_store::Store::open(&dir.path().join("test_voice.db")).unwrap());
        let (url, _seen) = crate::testkit::fake_server(vec![]).await;
        let (_worker, controls, _events) = crate::testkit::worker(&url, dir.path());

        let bus = Arc::new(Bus::default());
        let token = "test-token-voice-endpoints";
        let state = WebState {
            token: token.into(),
            controls,
            bus: bus.clone(),
            store: store.clone(),
            ollama: None,
            routing: Arc::new(std::sync::RwLock::new(
                oblivion_core::config::RoutingSettings::default(),
            )),
            config_path: None,
            voice_transcriber: Some(Arc::new(oblivion_core::MockTranscriber::new(
                "mock-voice-engine",
                "create a new workspace file",
            ))),
            voice_synthesizer: Some(Arc::new(oblivion_core::MockSynthesizer::new("mock-piper"))),
            wake_word_enabled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let (listener, addr) = bind(0).await.unwrap();
        let base = format!("http://{addr}");
        tokio::spawn(async move {
            axum::serve(listener, router(state)).await.unwrap();
        });

        let client = reqwest::Client::new();

        // 1. Voice status - unauthorized without token
        let res = client
            .get(format!("{base}/api/voice/status"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 2. Voice status - authorized via header
        let res = client
            .get(format!("{base}/api/voice/status"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let status: oblivion_core::VoiceStatus = res.json().await.unwrap();
        assert!(status.enabled);
        assert_eq!(status.hotkey, "Ctrl+Space");
        assert!(status.local_whisper_available);

        // 3. Voice transcribe - raw WAV audio
        let wav_bytes = make_test_wav(16000, 300);
        let res = client
            .post(format!("{base}/api/oblivion/voice/transcribe"))
            .header("x-jarvis-token", token)
            .header("content-type", "audio/wav")
            .body(wav_bytes.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let tr: VoiceTranscribeResponse = res.json().await.unwrap();
        assert_eq!(tr.text, "create a new workspace file");
        assert_eq!(tr.engine, "mock-voice-engine");
        assert_eq!(tr.duration_ms, 300);

        // 4. Voice transcribe - JSON base64 payload
        let b64 = base64::engine::general_purpose::STANDARD.encode(&wav_bytes);
        let res = client
            .post(format!("{base}/api/voice/transcribe?t={token}"))
            .json(&serde_json::json!({
                "audio_base64": b64,
                "language": "en"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let tr2: VoiceTranscribeResponse = res.json().await.unwrap();
        assert_eq!(tr2.text, "create a new workspace file");

        // 5. Voice transcribe - invalid audio returns 400 Bad Request
        let res = client
            .post(format!("{base}/api/voice/transcribe"))
            .header("x-oblivion-token", token)
            .body(vec![1, 2, 3, 4])
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);

        // 6. Voice TTS speak - unauthorized without token
        let res = client
            .post(format!("{base}/api/voice/tts/speak"))
            .json(&serde_json::json!({ "text": "Hello world" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 7. Voice TTS speak - authorized synthesis
        let res = client
            .post(format!("{base}/api/voice/tts/speak"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({ "text": "Hello world from Jarvis" }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let speak_resp: VoiceSpeakResponse = res.json().await.unwrap();
        assert_eq!(speak_resp.text, "Hello world from Jarvis");
        assert!(speak_resp.duration_ms >= 100);
        assert_eq!(speak_resp.format, "audio/wav");
        assert!(!speak_resp.audio_base64.is_empty());

        // 8. Wake word status - unauthorized without token
        let res = client
            .get(format!("{base}/api/voice/wake-word"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        // 9. Wake word status - authorized
        let res = client
            .get(format!("{base}/api/voice/wake-word"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ww_resp: serde_json::Value = res.json().await.unwrap();
        assert_eq!(ww_resp["enabled"], false);
        assert_eq!(ww_resp["phrases"][0], "jarvis");

        // 10. Wake word toggle - authorized POST
        let res = client
            .post(format!("{base}/api/oblivion/voice/wake-word"))
            .header("x-jarvis-token", token)
            .json(&serde_json::json!({ "enabled": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let ww_resp2: serde_json::Value = res.json().await.unwrap();
        assert_eq!(ww_resp2["enabled"], true);

        // 11. Send with image attachment
        let res = client
            .post(format!("{base}/send"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({
                "text": "What is in this image?",
                "images": [
                    {
                        "mime_type": "image/png",
                        "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
                        "name": "red_dot.png"
                    }
                ]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // 12. Chat handler with image attachment
        let res = client
            .post(format!("{base}/api/chat"))
            .header("x-oblivion-token", token)
            .json(&serde_json::json!({
                "message": "Analyze screen",
                "images": [
                    {
                        "mime_type": "image/jpeg",
                        "data_base64": "/9j/4AAQSkZJRg==",
                        "name": "screen.jpg"
                    }
                ]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // 13. Screen capture endpoint
        let res = client
            .post(format!("{base}/api/oblivion/screen/capture"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let screen_json: serde_json::Value = res.json().await.unwrap();
        assert!(screen_json.get("ok").is_some());
        if screen_json["ok"] == true {
            assert!(screen_json.get("image").is_some());
        } else {
            assert!(screen_json.get("error").is_some());
        }

        // 14. Computer use endpoints: status, stop, reset
        let res = client
            .get(format!("{base}/api/computer/status"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cu_status: serde_json::Value = res.json().await.unwrap();
        assert!(cu_status.get("enabled").is_some());
        assert_eq!(cu_status["stopped"], false);
        assert_eq!(cu_status["stop_hotkey"], "Escape");

        // Emergency stop
        let res = client
            .post(format!("{base}/api/computer/stop"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(oblivion_tools::is_emergency_stopped());

        // Check status reflects stopped
        let res = client
            .get(format!("{base}/api/oblivion/computer/status"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cu_status2: serde_json::Value = res.json().await.unwrap();
        assert_eq!(cu_status2["stopped"], true);

        // Emergency reset
        let res = client
            .post(format!("{base}/api/oblivion/computer/reset"))
            .header("x-oblivion-token", token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!oblivion_tools::is_emergency_stopped());
    }
}
