//! Tests for request classification and smart routing with offline mode.

use std::sync::Arc;

use oblivion_core::routing::{classify_request, RequestClass, RoutingRules};
use oblivion_providers::{ProviderConfig, Router, Target};

fn make_target(provider_name: &str, base_url: &str, model: &str) -> Target {
    Target::new(
        Arc::new(ProviderConfig {
            name: provider_name.to_string(),
            base_url: base_url.to_string(),
            api_key: None,
            headers: Vec::new(),
            stream_usage: false,
        }),
        model,
    )
}

#[test]
fn test_target_is_local() {
    let t_ollama = make_target("ollama", "http://localhost:11434/v1", "qwen2.5-coder:7b");
    assert!(t_ollama.is_local());

    let t_local_host = make_target("custom", "http://127.0.0.1:8000/v1", "mistral:7b");
    assert!(t_local_host.is_local());

    let t_cloud = make_target("groq", "https://api.groq.com/openai/v1", "llama-3.3-70b");
    assert!(!t_cloud.is_local());

    let t_openrouter = make_target(
        "openrouter",
        "https://openrouter.ai/api/v1",
        "anthropic/claude-3.5-sonnet",
    );
    assert!(!t_openrouter.is_local());
}

#[test]
fn test_router_for_patterns_offline_purges_cloud() {
    let t_cloud1 = make_target("groq", "https://api.groq.com/openai/v1", "llama-3.3-70b");
    let t_local = make_target("ollama", "http://localhost:11434/v1", "qwen2.5-coder:7b");
    let t_cloud2 = make_target("openrouter", "https://openrouter.ai/api/v1", "claude");

    let router = Router::new(vec![t_cloud1, t_local, t_cloud2]);
    let rules = RoutingRules::default();

    // Offline mode: only local target remains
    let offline_router = router.share().for_patterns(&rules.simple, true);
    assert_eq!(offline_router.targets().len(), 1);
    assert_eq!(
        offline_router.targets()[0].label(),
        "ollama/qwen2.5-coder:7b"
    );
}

#[test]
fn test_router_for_patterns_reorders_by_priority() {
    let t_groq_instant = make_target(
        "groq",
        "https://api.groq.com/openai/v1",
        "llama-3.1-8b-instant",
    );
    let t_groq_heavy = make_target(
        "groq",
        "https://api.groq.com/openai/v1",
        "llama-3.3-70b-versatile",
    );
    let t_ollama = make_target("ollama", "http://localhost:11434/v1", "qwen2.5-coder:7b");

    let router = Router::new(vec![
        t_groq_heavy.clone(),
        t_groq_instant.clone(),
        t_ollama.clone(),
    ]);
    let rules = RoutingRules::default();

    // For Hard class: groq/llama-3.3-70b-versatile should be first
    let hard_router = router.share().for_patterns(&rules.hard, false);
    assert_eq!(
        hard_router.targets()[0].label(),
        "groq/llama-3.3-70b-versatile"
    );

    // For Simple class: rules.simple starts with ollama/* then groq/llama-3.1-8b-instant
    let simple_router = router.share().for_patterns(&rules.simple, false);
    assert_eq!(
        simple_router.targets()[0].label(),
        "ollama/qwen2.5-coder:7b"
    );
    assert_eq!(
        simple_router.targets()[1].label(),
        "groq/llama-3.1-8b-instant"
    );
}

#[test]
fn test_classification_sensitive_credentials() {
    let class = classify_request("Check my AWS_ACCESS_KEY_ID in .env", 100, false, false);
    assert_eq!(class, RequestClass::Private);

    let class2 = classify_request("Here is the database password: admin123", 100, false, false);
    assert_eq!(class2, RequestClass::Private);
}

#[test]
fn test_classification_offline_flag() {
    let class = classify_request("A completely simple question", 50, false, true);
    assert_eq!(class, RequestClass::Private);
}
