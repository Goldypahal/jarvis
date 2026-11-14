//! Streaming client for any OpenAI-compatible `/chat/completions` endpoint.

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::types::{Completion, FunctionCall, Message, ToolCall, ToolSpec, Usage};

/// Connection details for one provider.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub name: String,
    /// e.g. `https://openrouter.ai/api/v1` (the client appends `/chat/completions`).
    pub base_url: String,
    pub api_key: Option<String>,
    /// Extra HTTP headers (OpenRouter likes `HTTP-Referer` and `X-Title`).
    pub headers: Vec<(String, String)>,
    /// Ask the provider to include token usage in the stream.
    pub stream_usage: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("rate limited{}: {body}", retry_note(*.retry_after_secs))]
    RateLimited {
        retry_after_secs: Option<u64>,
        body: String,
    },
    #[error("authentication failed ({status}): {body}")]
    Auth { status: u16, body: String },
    #[error("request rejected ({status}): {body}")]
    BadRequest { status: u16, body: String },
    #[error("server error ({status}): {body}")]
    Server { status: u16, body: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("bad response from provider: {0}")]
    Protocol(String),
}

/// Something that happens while a completion streams in.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    TextDelta(String),
    /// A target failed and the router moved on to the next one.
    Fallback {
        from: String,
        reason: String,
    },
    /// Every model is briefly rate-limited; the router waits this long, then retries.
    Waiting {
        seconds: u64,
    },
}

fn retry_note(secs: Option<u64>) -> String {
    match secs {
        Some(s) => format!(" (retry in {s}s)"),
        None => String::new(),
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

impl Client {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(180))
            .user_agent(concat!("oblivion/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client");
        Self { http }
    }

    /// Run one streamed chat completion and assemble the full assistant message.
    pub async fn complete(
        &self,
        provider: &ProviderConfig,
        model: &str,
        messages: &[Message],
        tools: &[ToolSpec],
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, ProviderError> {
        let url = format!(
            "{}/chat/completions",
            provider.base_url.trim_end_matches('/')
        );
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": true,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(ToolSpec::to_wire).collect());
            body["tool_choice"] = json!("auto");
        }
        if provider.stream_usage {
            body["stream_options"] = json!({ "include_usage": true });
        }

        let mut req = self.http.post(&url).json(&body);
        if let Some(key) = &provider.api_key {
            req = req.bearer_auth(key);
        }
        for (k, v) in &provider.headers {
            req = req.header(k, v);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok());
            let text = truncate(resp.text().await.unwrap_or_default().trim(), 600);
            return Err(classify(status.as_u16(), retry_after, text));
        }

        let mut acc = Accumulator::default();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        'outer: while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Network(e.to_string()))?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line: String = buf.drain(..=pos).collect();
                match acc.feed_line(line.trim_end_matches(['\r', '\n']), on_event)? {
                    LineOutcome::Continue => {}
                    LineOutcome::Done => break 'outer,
                }
            }
        }
        // A final line without a trailing newline.
        if !buf.trim().is_empty() {
            acc.feed_line(buf.trim(), on_event)?;
        }
        acc.finish(format!("{}/{}", provider.name, model))
    }
}

fn classify(status: u16, retry_after_secs: Option<u64>, body: String) -> ProviderError {
    match status {
        429 => ProviderError::RateLimited {
            retry_after_secs,
            body,
        },
        401..=403 => ProviderError::Auth { status, body },
        400..=499 => ProviderError::BadRequest { status, body },
        _ => ProviderError::Server { status, body },
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

enum LineOutcome {
    Continue,
    Done,
}

#[derive(Default)]
struct PartialCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

/// Builds one assistant message from SSE chunks.
#[derive(Default)]
struct Accumulator {
    text: String,
    calls: BTreeMap<u64, PartialCall>,
    finish_reason: Option<String>,
    usage: Option<Usage>,
    saw_data: bool,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    #[serde(default)]
    usage: Option<Usage>,
    #[serde(default)]
    error: Option<Value>,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Option<Delta>,
    /// Some providers send a full `message` instead of `delta` on the last chunk.
    #[serde(default)]
    message: Option<Delta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Deserialize)]
struct DeltaToolCall {
    #[serde(default)]
    index: Option<u64>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

impl Accumulator {
    fn feed_line(
        &mut self,
        line: &str,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<LineOutcome, ProviderError> {
        // SSE comments (": keep-alive") and event names carry nothing we need.
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(LineOutcome::Continue);
        };
        let data = data.trim();
        if data.is_empty() {
            return Ok(LineOutcome::Continue);
        }
        if data == "[DONE]" {
            return Ok(LineOutcome::Done);
        }
        self.saw_data = true;
        let chunk: Chunk = serde_json::from_str(data)
            .map_err(|e| ProviderError::Protocol(format!("{e}: {}", truncate(data, 200))))?;
        if let Some(err) = chunk.error {
            // The model produced an invalid tool call (e.g. a tool that does not exist).
            // That is about this generation, not the provider's health: no cooldown.
            if err.to_string().contains("tool_use_failed") {
                return Err(ProviderError::BadRequest {
                    status: 400,
                    body: truncate(&err.to_string(), 300),
                });
            }
            return Err(ProviderError::Server {
                status: 200,
                body: truncate(&err.to_string(), 600),
            });
        }
        if let Some(u) = chunk.usage {
            self.usage = Some(u);
        }
        for choice in chunk.choices {
            if let Some(reason) = choice.finish_reason {
                self.finish_reason = Some(reason);
            }
            for delta in [choice.delta, choice.message].into_iter().flatten() {
                if let Some(text) = delta.content {
                    if !text.is_empty() {
                        on_event(StreamEvent::TextDelta(text.clone()));
                        self.text.push_str(&text);
                    }
                }
                for (pos, tc) in delta.tool_calls.unwrap_or_default().into_iter().enumerate() {
                    let index = tc.index.unwrap_or(pos as u64);
                    let entry = self.calls.entry(index).or_default();
                    if let Some(id) = tc.id {
                        if !id.is_empty() {
                            entry.id = Some(id);
                        }
                    }
                    if let Some(f) = tc.function {
                        if let Some(name) = f.name {
                            // Most providers send the name once; a few repeat it.
                            if !entry.name.ends_with(&name) {
                                entry.name.push_str(&name);
                            }
                        }
                        if let Some(args) = f.arguments {
                            entry.arguments.push_str(&args);
                        }
                    }
                }
            }
        }
        Ok(LineOutcome::Continue)
    }

    fn finish(self, served_by: String) -> Result<Completion, ProviderError> {
        if !self.saw_data {
            return Err(ProviderError::Protocol("empty response stream".into()));
        }
        let tool_calls: Vec<ToolCall> = self
            .calls
            .into_iter()
            .filter(|(_, c)| !c.name.is_empty())
            .map(|(i, c)| ToolCall {
                id: c.id.unwrap_or_else(|| format!("call_{i}")),
                kind: "function".into(),
                function: FunctionCall {
                    name: c.name,
                    arguments: if c.arguments.trim().is_empty() {
                        "{}".into()
                    } else {
                        c.arguments
                    },
                },
            })
            .collect();
        let content = if self.text.is_empty() {
            None
        } else {
            Some(self.text)
        };
        Ok(Completion {
            message: Message::assistant(content, tool_calls),
            finish_reason: self.finish_reason,
            usage: self.usage,
            served_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> (Completion, Vec<StreamEvent>) {
        let mut acc = Accumulator::default();
        let mut events = vec![];
        let mut sink = |e: StreamEvent| events.push(e);
        for l in lines {
            if let LineOutcome::Done = acc.feed_line(l, &mut sink).unwrap() {
                break;
            }
        }
        (acc.finish("p/m".into()).unwrap(), events)
    }

    #[test]
    fn assembles_text() {
        let (c, ev) = run(&[
            r#"data: {"choices":[{"delta":{"content":"Hel"}}]}"#,
            ": keep-alive",
            r#"data: {"choices":[{"delta":{"content":"lo"},"finish_reason":"stop"}]}"#,
            "data: [DONE]",
        ]);
        assert_eq!(c.message.content.as_deref(), Some("Hello"));
        assert_eq!(c.finish_reason.as_deref(), Some("stop"));
        assert_eq!(ev.len(), 2);
    }

    #[test]
    fn assembles_split_tool_calls() {
        let (c, _) = run(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"x\"}"}}]}}]}"#,
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"a2","function":{"name":"bash","arguments":""}}]}}]}"#,
            r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
            "data: [DONE]",
        ]);
        let calls = &c.message.tool_calls;
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "a1");
        assert_eq!(calls[0].function.arguments, r#"{"path":"x"}"#);
        assert_eq!(calls[1].function.name, "bash");
        assert_eq!(calls[1].function.arguments, "{}");
        assert!(c.message.content.is_none());
    }

    #[test]
    fn missing_ids_get_generated() {
        let (c, _) = run(&[
            r#"data: {"choices":[{"delta":{"tool_calls":[{"function":{"name":"x","arguments":"{}"}}]}}]}"#,
        ]);
        assert_eq!(c.message.tool_calls[0].id, "call_0");
    }

    #[test]
    fn stream_error_is_reported() {
        let mut acc = Accumulator::default();
        let mut sink = |_e: StreamEvent| {};
        let r = acc.feed_line(r#"data: {"error":{"message":"overloaded"}}"#, &mut sink);
        assert!(matches!(r, Err(ProviderError::Server { .. })));
    }

    #[test]
    fn classify_statuses() {
        assert!(matches!(
            classify(429, Some(3), String::new()),
            ProviderError::RateLimited {
                retry_after_secs: Some(3),
                ..
            }
        ));
        assert!(matches!(
            classify(401, None, String::new()),
            ProviderError::Auth { .. }
        ));
        assert!(matches!(
            classify(404, None, String::new()),
            ProviderError::BadRequest { .. }
        ));
        assert!(matches!(
            classify(503, None, String::new()),
            ProviderError::Server { .. }
        ));
    }
}
