//! Request classification and smart routing across model tiers.
//!
//! Categories:
//! - `Private`: Sensitive content (passwords, tokens, private keys) or offline mode.
//!   Cloud models are completely excluded; routes strictly to local models (Ollama).
//! - `Simple`: Straightforward queries, quick answers, fast edits.
//! - `Hard`: Complex refactorings, multi-file architectural work, tricky debugging.
//! - `Vision`: Image or visual analysis requests.
//! - `LongContext`: Requests with large context history (> 16K tokens).

use serde::{Deserialize, Serialize};

/// Categories a user request can fall into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestClass {
    /// Sensitive content (passwords, tokens, private keys, local-only).
    Private,
    /// Fast, straightforward questions, quick edits, small explanations.
    Simple,
    /// Complex multi-step reasoning, architectural redesigns, debugging, refactoring.
    Hard,
    /// Involves image or visual analysis.
    Vision,
    /// Conversation or context exceeds standard context limits (> 16K tokens).
    LongContext,
}

impl RequestClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Simple => "simple",
            Self::Hard => "hard",
            Self::Vision => "vision",
            Self::LongContext => "long_context",
        }
    }
}

/// Routing rules for each request class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingRules {
    #[serde(default = "default_private_rules")]
    pub private: Vec<String>,
    #[serde(default = "default_simple_rules")]
    pub simple: Vec<String>,
    #[serde(default = "default_hard_rules")]
    pub hard: Vec<String>,
    #[serde(default = "default_vision_rules")]
    pub vision: Vec<String>,
    #[serde(default = "default_long_context_rules")]
    pub long_context: Vec<String>,
}

pub fn default_private_rules() -> Vec<String> {
    vec!["ollama/*".into(), "local/*".into()]
}

pub fn default_simple_rules() -> Vec<String> {
    vec![
        "ollama/*".into(),
        "groq/llama-3.1-8b-instant".into(),
        "openrouter/*".into(),
    ]
}

pub fn default_hard_rules() -> Vec<String> {
    vec![
        "groq/llama-3.3-70b-versatile".into(),
        "openrouter/*".into(),
        "ollama/*".into(),
    ]
}

pub fn default_vision_rules() -> Vec<String> {
    vec![
        "openrouter/*".into(),
        "ollama/llava*".into(),
        "ollama/*".into(),
    ]
}

pub fn default_long_context_rules() -> Vec<String> {
    vec!["openrouter/*".into(), "groq/*".into(), "ollama/*".into()]
}

impl Default for RoutingRules {
    fn default() -> Self {
        Self {
            private: default_private_rules(),
            simple: default_simple_rules(),
            hard: default_hard_rules(),
            vision: default_vision_rules(),
            long_context: default_long_context_rules(),
        }
    }
}

impl RoutingRules {
    pub fn patterns_for(&self, class: RequestClass) -> &[String] {
        match class {
            RequestClass::Private => &self.private,
            RequestClass::Simple => &self.simple,
            RequestClass::Hard => &self.hard,
            RequestClass::Vision => &self.vision,
            RequestClass::LongContext => &self.long_context,
        }
    }
}

/// Classify a user turn based on content, context size, image attachments, and offline status.
pub fn classify_request(
    text: &str,
    history_tokens: usize,
    has_images: bool,
    offline: bool,
) -> RequestClass {
    // 1. If offline mode is globally enabled, every request is strictly Private (local-only)
    if offline {
        return RequestClass::Private;
    }

    // 2. Vision input
    if has_images {
        return RequestClass::Vision;
    }

    // 3. Long context (> 16,000 tokens)
    if history_tokens >= 16_000 {
        return RequestClass::LongContext;
    }

    let lower = text.to_ascii_lowercase();

    // 4. Sensitive & Private indicators
    const PRIVATE_PATTERNS: &[&str] = &[
        "password",
        "secret",
        "credential",
        "private_key",
        "private key",
        "api_key",
        "api key",
        "access_token",
        "access token",
        "auth_token",
        "auth token",
        "bearer ",
        "id_rsa",
        "id_ed25519",
        "ssh-rsa",
        ".env",
        "ssn",
        "seed phrase",
        "confidential",
        "/private",
        "offline only",
        "local only",
        "don't send to cloud",
        "do not send to cloud",
    ];

    if PRIVATE_PATTERNS.iter().any(|pat| lower.contains(pat)) {
        return RequestClass::Private;
    }

    // 5. Hard / complex reasoning indicators
    const HARD_PATTERNS: &[&str] = &[
        "refactor",
        "architect",
        "redesign",
        "debug",
        "deadlock",
        "race condition",
        "memory leak",
        "security review",
        "security audit",
        "comprehensive test",
        "rewrite",
        "optimize algorithm",
        "deep analysis",
    ];

    let has_hard_word = HARD_PATTERNS.iter().any(|pat| lower.contains(pat));
    let has_code_fence = text.contains("```");
    let is_lengthy_code_prompt = has_code_fence && text.len() > 700;

    if has_hard_word || is_lengthy_code_prompt {
        return RequestClass::Hard;
    }

    // 6. Otherwise standard/simple
    RequestClass::Simple
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_offline_forces_private() {
        assert_eq!(
            classify_request("Hello, what is the weather?", 100, false, true),
            RequestClass::Private
        );
    }

    #[test]
    fn test_classify_vision() {
        assert_eq!(
            classify_request("Describe this image", 200, true, false),
            RequestClass::Vision
        );
    }

    #[test]
    fn test_classify_long_context() {
        assert_eq!(
            classify_request("Summarize our discussion", 18_000, false, false),
            RequestClass::LongContext
        );
    }

    #[test]
    fn test_classify_sensitive_private() {
        assert_eq!(
            classify_request("Here is my API_KEY=xyz123, test this", 500, false, false),
            RequestClass::Private
        );
        assert_eq!(
            classify_request("Read the .env file and check passwords", 500, false, false),
            RequestClass::Private
        );
        assert_eq!(
            classify_request("Please keep this offline only", 200, false, false),
            RequestClass::Private
        );
    }

    #[test]
    fn test_classify_hard() {
        assert_eq!(
            classify_request(
                "Refactor the memory storage layer to handle concurrency",
                400,
                false,
                false
            ),
            RequestClass::Hard
        );
        assert_eq!(
            classify_request(
                "Perform a security review of our authorization handlers",
                400,
                false,
                false
            ),
            RequestClass::Hard
        );
    }

    #[test]
    fn test_classify_simple() {
        assert_eq!(
            classify_request("What is the capital of France?", 100, false, false),
            RequestClass::Simple
        );
        assert_eq!(
            classify_request(
                "Explain the difference between a mutex and a semaphore in one paragraph",
                300,
                false,
                false
            ),
            RequestClass::Simple
        );
    }
}
