//! End-of-session memory extraction pass (SRS Month 2).
//!
//! Analyzes completed session conversations to extract durable facts, user preferences,
//! project facts, recipes, and past outcomes. Uses a budgeted cheap-model pass
//! (`routing.summary` preference) and falls back to deterministic heuristic patterns
//! if offline or if model inference fails.

use oblivion_providers::{Message, Role, Router, StreamEvent};
use serde::{Deserialize, Serialize};

pub const EXTRACT_PROMPT: &str = r#"You are a memory extraction assistant. Analyze the transcript of a completed conversation between a USER and an ASSISTANT.
Identify durable facts, user preferences, project facts, recipes, or past outcomes that will be valuable across future sessions.

Extract only durable, high-value information:
- "user_pref": personal preferences (e.g., style, formatting, preferred tools, workflows)
- "project_fact": permanent facts about the repository, codebase, or architecture
- "fact": durable facts about the user or environment
- "recipe": reusable procedural instructions discovered or tested
- "past_outcome": significant lessons learned or fixed issues

Rules:
1. Do NOT extract transient conversation state (e.g. "user asked to fix line 4", "agent read file x").
2. Output each extracted memory as a single line JSON object matching this schema:
   {"type": "user_pref|project_fact|fact|recipe|past_outcome", "text": "concise declarative statement", "importance": 1-5}
3. If there are no durable facts to remember, output nothing.
4. Do not include markdown code fences or conversational preambles. Output raw JSON lines only."#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProposedMemory {
    #[serde(rename = "type")]
    pub mem_type: String,
    pub text: String,
    pub importance: u32,
}

pub type RecallFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Vec<String>> + Send + 'a>>;

/// Trait for retrieving relevant memories to inject into prompt context.
pub trait MemoryRecall: Send + Sync {
    /// Retrieve formatted memory strings relevant to `prompt` within `budget_chars`
    /// scoped to `project` (so project facts never leak to other projects).
    fn recall_for_prompt<'a>(
        &'a self,
        prompt: &'a str,
        project: &'a str,
        budget_chars: usize,
    ) -> RecallFuture<'a>;
}

impl ProposedMemory {
    pub fn new(mem_type: impl Into<String>, text: impl Into<String>, importance: u32) -> Self {
        let mt = mem_type.into();
        let valid_type = match mt.as_str() {
            "user_pref" | "project_fact" | "fact" | "recipe" | "past_outcome" => mt,
            _ => "fact".to_string(),
        };
        Self {
            mem_type: valid_type,
            text: text.into(),
            importance: importance.clamp(1, 5),
        }
    }
}

/// Parse model output into structured memory proposals.
/// Resilient against markdown code fences, JSON arrays, and trailing text.
pub fn parse_proposals(raw: &str) -> Vec<ProposedMemory> {
    let mut proposals = Vec::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return proposals;
    }

    // Try parsing as a full JSON array first
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        if let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(trimmed) {
            for v in items {
                if let Some(p) = value_to_proposed(&v) {
                    proposals.push(p);
                }
            }
            if !proposals.is_empty() {
                return proposals;
            }
        }
    }

    // Otherwise parse line by line (JSON lines or embedded objects)
    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("```")
            || line.starts_with("//")
            || line.starts_with('#')
        {
            continue;
        }

        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(p) = value_to_proposed(&v) {
                proposals.push(p);
                continue;
            }
        }

        // Try extracting embedded JSON object substring `{...}`
        if let (Some(start), Some(end)) = (line.find('{'), line.rfind('}')) {
            if start < end {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line[start..=end]) {
                    if let Some(p) = value_to_proposed(&v) {
                        proposals.push(p);
                    }
                }
            }
        }
    }

    proposals
}

fn value_to_proposed(v: &serde_json::Value) -> Option<ProposedMemory> {
    let text = v.get("text").and_then(|t| t.as_str())?.trim();
    if text.is_empty() {
        return None;
    }
    let mem_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("fact");
    let importance = v.get("importance").and_then(|i| i.as_u64()).unwrap_or(3) as u32;

    Some(ProposedMemory::new(mem_type, text, importance))
}

/// Fast heuristic memory extractor when no model is available or model extraction returns nothing.
pub fn extract_heuristics(messages: &[Message]) -> Vec<ProposedMemory> {
    let mut proposals = Vec::new();
    let mut seen_texts = std::collections::HashSet::new();

    for m in messages {
        if m.role != Role::User {
            continue;
        }
        let Some(ref content) = m.content else {
            continue;
        };

        for sentence in content.split(['\n', '.', ';', '!', '?']) {
            let s = sentence.trim();
            if s.is_empty() {
                continue;
            }
            let s_lower = s.to_lowercase();

            let proposal = if s_lower.starts_with("i prefer ") {
                let rest = s[9..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "user_pref",
                        format!("User prefers {rest}"),
                        4,
                    ))
                } else {
                    None
                }
            } else if s_lower.starts_with("always use ") {
                let rest = s[11..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "user_pref",
                        format!("Always use {rest}"),
                        5,
                    ))
                } else {
                    None
                }
            } else if s_lower.starts_with("never use ") {
                let rest = s[10..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "user_pref",
                        format!("Never use {rest}"),
                        5,
                    ))
                } else {
                    None
                }
            } else if s_lower.starts_with("my project is ") {
                let rest = s[14..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "project_fact",
                        format!("Active project is {rest}"),
                        4,
                    ))
                } else {
                    None
                }
            } else if s_lower.starts_with("my project uses ") {
                let rest = s[16..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "project_fact",
                        format!("Project uses {rest}"),
                        4,
                    ))
                } else {
                    None
                }
            } else if s_lower.starts_with("remember that ") {
                let rest = s[14..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new("fact", rest, 4))
                } else {
                    None
                }
            } else if s_lower.starts_with("remember: ") {
                let rest = s[10..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new("fact", rest, 4))
                } else {
                    None
                }
            } else if s_lower.starts_with("my name is ") {
                let rest = s[11..].trim().trim_end_matches(['.', '!', ',']);
                if !rest.is_empty() {
                    Some(ProposedMemory::new(
                        "fact",
                        format!("User's name is {rest}"),
                        5,
                    ))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(p) = proposal {
                let key = p.text.to_lowercase();
                if !seen_texts.contains(&key) {
                    seen_texts.insert(key);
                    proposals.push(p);
                }
            }
        }
    }

    proposals
}

/// Execute the extraction pass using a cheap model with a capped token budget.
pub async fn extract_memories<F>(
    router: &Router,
    prefer_model: &str,
    messages: &[Message],
    budget_chars: usize,
    mut on_fallback: F,
) -> Vec<ProposedMemory>
where
    F: FnMut(String, String) + Send,
{
    let transcript = crate::compact::transcript(messages, budget_chars);
    if transcript.trim().is_empty() {
        return Vec::new();
    }

    let request = vec![Message::system(EXTRACT_PROMPT), Message::user(transcript)];

    let mut quiet = |e: StreamEvent| {
        if let StreamEvent::Fallback { from, reason } = e {
            on_fallback(from, reason);
        }
    };

    let mut extractor = router.share().preferring(prefer_model);
    let output = match extractor.complete(&request, &[], &mut quiet).await {
        Ok(c) => c.message.content.unwrap_or_default(),
        Err(_) => String::new(),
    };

    let mut memories = parse_proposals(&output);
    if memories.is_empty() {
        memories = extract_heuristics(messages);
    }
    memories
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parse_json_lines() {
        let raw = r#"
{"type": "user_pref", "text": "User prefers dark theme", "importance": 4}
{"type": "project_fact", "text": "Database is SQLite FTS5", "importance": 5}
{"type": "invalid_type", "text": "Something else", "importance": 10}
"#;
        let res = parse_proposals(raw);
        assert_eq!(res.len(), 3);
        assert_eq!(res[0].mem_type, "user_pref");
        assert_eq!(res[0].text, "User prefers dark theme");
        assert_eq!(res[0].importance, 4);

        assert_eq!(res[1].mem_type, "project_fact");
        assert_eq!(res[1].importance, 5);

        // invalid type fell back to fact, importance clamped to 5
        assert_eq!(res[2].mem_type, "fact");
        assert_eq!(res[2].importance, 5);
    }

    #[test]
    fn parse_json_array_with_markdown_fences() {
        let raw = r#"```json
[
  {"type": "recipe", "text": "Run cargo test --workspace before commit", "importance": 5},
  {"type": "user_pref", "text": "Keep responses short", "importance": 4}
]
```"#;
        let res = parse_proposals(raw);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].mem_type, "recipe");
        assert_eq!(res[1].mem_type, "user_pref");
    }

    #[test]
    fn heuristic_extraction_detects_patterns() {
        let messages = vec![
            Message::user("Hello! My name is Alice. I prefer dark theme and concise code."),
            Message::assistant(Some("Nice to meet you Alice.".into()), vec![]),
            Message::user(
                "My project uses Rust 2024. Remember that API keys are stored in keyring.",
            ),
        ];

        let res = extract_heuristics(&messages);
        assert!(res.iter().any(|m| m.text == "User's name is Alice"));
        assert!(res
            .iter()
            .any(|m| m.text.contains("User prefers dark theme")));
        assert!(res.iter().any(|m| m.text == "Project uses Rust 2024"));
        assert!(res
            .iter()
            .any(|m| m.text == "API keys are stored in keyring"));
    }

    #[tokio::test]
    async fn extraction_with_model_response() {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let req_str = String::from_utf8_lossy(&buf[..n]);
            seen_clone.lock().unwrap().push(req_str.to_string());

            let content_str = "{\"type\": \"user_pref\", \"text\": \"User prefers 4 spaces over tabs\", \"importance\": 4}\n{\"type\": \"project_fact\", \"text\": \"Project uses Actix-web\", \"importance\": 3}";
            let escaped = serde_json::to_string(content_str).unwrap();
            let chunk = format!(r#"{{"choices":[{{"delta":{{"content":{escaped}}}}}]}}"#);
            let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
        });

        let target = oblivion_providers::Target {
            provider: Arc::new(oblivion_providers::ProviderConfig {
                name: "test-prov".into(),
                base_url: format!("http://{addr}"),
                api_key: Some("key".into()),
                headers: Default::default(),
                stream_usage: false,
            }),
            model: "model-1".into(),
            requests_per_day: None,
            max_input_tokens: None,
            checked: None,
        };
        let router = Router::new(vec![target]);

        let messages = vec![
            Message::user("Please format code with 4 spaces. Our project uses Actix-web."),
            Message::assistant(Some("Understood.".into()), vec![]),
        ];

        let extracted = extract_memories(&router, "", &messages, 4000, |_, _| {}).await;
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0].mem_type, "user_pref");
        assert_eq!(extracted[0].text, "User prefers 4 spaces over tabs");
        assert_eq!(extracted[1].mem_type, "project_fact");
        assert_eq!(extracted[1].text, "Project uses Actix-web");

        // Verify request was sent
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn extraction_fallback_to_heuristics_when_model_fails() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        });

        let target = oblivion_providers::Target {
            provider: Arc::new(oblivion_providers::ProviderConfig {
                name: "fail-prov".into(),
                base_url: format!("http://{addr}"),
                api_key: Some("key".into()),
                headers: Default::default(),
                stream_usage: false,
            }),
            model: "model-fail".into(),
            requests_per_day: None,
            max_input_tokens: None,
            checked: None,
        };
        let router = Router::new(vec![target]);

        let messages = vec![Message::user(
            "I prefer neovim over vscode. My project uses Rust.",
        )];

        let extracted = extract_memories(&router, "", &messages, 4000, |_, _| {}).await;
        // Fell back to heuristic extraction!
        assert!(!extracted.is_empty());
        assert!(extracted
            .iter()
            .any(|m| m.text == "User prefers neovim over vscode"));
    }

    #[tokio::test]
    async fn agent_disable_memory_extraction() {
        struct YesApprover;
        impl crate::Approver for YesApprover {
            fn approve(&self, _tool: &str, _summary: &str) -> crate::Decision {
                crate::Decision::Yes
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let tb = oblivion_tools::ToolBox::new(
            dir.path(),
            oblivion_tools::Shell::Sh,
            dir.path().join(".snap"),
        );
        let target = oblivion_providers::Target {
            provider: Arc::new(oblivion_providers::ProviderConfig {
                name: "dummy".into(),
                base_url: "http://127.0.0.1:9".into(),
                api_key: Some("k".into()),
                headers: Default::default(),
                stream_usage: false,
            }),
            model: "m".into(),
            requests_per_day: None,
            max_input_tokens: None,
            checked: None,
        };
        let mut agent = crate::Agent::new(
            Router::new(vec![target]),
            tb,
            Box::new(YesApprover),
            crate::ApprovalMode::Yolo,
            10,
            "sys".into(),
        );
        agent
            .session
            .messages
            .push(Message::user("I prefer dark mode"));
        agent
            .session
            .messages
            .push(Message::assistant(Some("ok".into()), vec![]));

        // When enabled, heuristics extract preferences
        agent.memory_auto_extract = true;
        let p_enabled = agent.extract_end_of_session_memories().await;
        assert!(!p_enabled.is_empty());

        // When disabled, returns empty list immediately
        agent.memory_auto_extract = false;
        let p_disabled = agent.extract_end_of_session_memories().await;
        assert!(p_disabled.is_empty());
    }

    #[test]
    fn config_parses_memory_settings_and_supports_disabling() {
        let toml_str = r#"
[agent]
max_steps = 10

[memory]
enabled = true
auto_extract = false
max_tokens = 1500
"#;
        let cfg: crate::Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.memory.enabled);
        assert!(!cfg.memory.auto_extract);
        assert_eq!(cfg.memory.max_tokens, 1500);
    }
}
