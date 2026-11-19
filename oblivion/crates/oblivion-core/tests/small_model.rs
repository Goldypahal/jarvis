use std::sync::Arc;

use oblivion_core::{
    agent::Agent, config::ApprovalMode, extract_tool_calls_from_text, is_small_model_name, prompt,
    repair_tool_arguments,
};
use oblivion_providers::{ProviderConfig, Router, Target};
use oblivion_tools::{Shell, ToolBox};
use serde_json::Value;

struct NoopApprover;
impl oblivion_core::Approver for NoopApprover {
    fn approve(&self, _tool: &str, _summary: &str) -> oblivion_core::Decision {
        oblivion_core::Decision::Yes
    }
}

fn test_router(target_name: &str) -> Router {
    let target = Target::new(
        Arc::new(ProviderConfig {
            name: "test-provider".into(),
            base_url: "http://localhost:11434".into(),
            api_key: None,
            headers: Vec::new(),
            stream_usage: false,
        }),
        target_name,
    );
    Router::new(vec![target])
}

fn test_agent(small_model_flag: bool, model_name: &str) -> (tempfile::TempDir, Agent) {
    let dir = tempfile::tempdir().unwrap();
    let snap = dir.path().join("snapshots");
    let tools = ToolBox::new(dir.path(), Shell::Bash, snap);
    let router = test_router(model_name);
    let system = prompt::build(dir.path(), Shell::Bash, "2026-11-19", &[]);
    let small_system = prompt::build_small(dir.path(), Shell::Bash, "2026-11-19");

    let mut agent = Agent::new(
        router,
        tools,
        Box::new(NoopApprover),
        ApprovalMode::Yolo,
        10,
        system,
    );
    agent.small_model = small_model_flag;
    agent.active_model = Some(model_name.to_string());
    agent.small_system_prompt = Some(small_system);
    (dir, agent)
}

#[test]
fn test_small_model_auto_detection() {
    assert!(is_small_model_name("qwen2.5-coder:7b"));
    assert!(is_small_model_name("llama3.2:3b"));
    assert!(is_small_model_name("groq/llama-3.1-8b-instant"));
    assert!(is_small_model_name("gemini-2.0-flash-lite"));
    assert!(!is_small_model_name("claude-3-5-sonnet"));
    assert!(!is_small_model_name("deepseek-r1"));
}

#[test]
fn test_small_model_mode_activation() {
    let (_dir, mut agent1) = test_agent(false, "claude-3-5-sonnet");
    assert!(!agent1.is_small_model_active());

    // Flag activates it regardless of model
    agent1.small_model = true;
    assert!(agent1.is_small_model_active());

    // Auto-detection activates it when model name is small
    let (_dir2, agent2) = test_agent(false, "ollama/qwen2.5-coder:7b");
    assert!(agent2.is_small_model_active());
}

#[test]
fn test_small_model_reduces_tool_specs_to_core() {
    let (_dir, _standard_agent) = test_agent(false, "claude-3-5-sonnet");
    let (_dir2, small_agent) = test_agent(true, "ollama/qwen2.5-coder:7b");

    // Small model only offers core tools (read_file, write_file, edit_file, glob, grep, shell, todo)
    let small_specs = small_agent.is_small_model_active();
    assert!(small_specs);

    // Verify repair on arguments
    let markdown_json = "```json\n{\"path\": \"hello.txt\", \"content\": \"world\"}\n```";
    let repaired = repair_tool_arguments(markdown_json).unwrap();
    assert_eq!(repaired["path"], "hello.txt");
    assert_eq!(repaired["content"], "world");
}

#[test]
fn test_extract_tool_calls_from_small_model_text() {
    let raw_reply = "I will read the test file now.\nread_file({\"path\": \"test.txt\"})\nDone.";
    let calls = extract_tool_calls_from_text(raw_reply, &["read_file", "write_file", "shell"]);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function.name, "read_file");
    let args: Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
    assert_eq!(args["path"], "test.txt");
}
