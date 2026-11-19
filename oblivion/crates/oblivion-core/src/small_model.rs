//! Adaptations and repair utilities for small local/cloud models (B13).
//!
//! Small models (1B-8B parameters, such as Qwen 2.5 7B, Llama 3.2 3B, Llama 3.1 8B)
//! frequently struggle with verbose prompts, complex tool schemas, and strict JSON formatting.
//! This module provides:
//! 1. Detection of small model identifiers.
//! 2. Robust output checks and argument repairing (unwrapping markdown fences, extracting JSON).
//! 3. Extraction of tool calls emitted directly into assistant message text.

use oblivion_providers::{FunctionCall, ToolCall};
use serde_json::Value;

/// Determines if a model label or identifier corresponds to a small model.
pub fn is_small_model_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains(":1b")
        || lower.contains(":3b")
        || lower.contains(":7b")
        || lower.contains(":8b")
        || lower.contains(":9b")
        || lower.contains("mini")
        || lower.contains("flash-lite")
        || lower.contains("haiku")
        || lower.contains("instant")
        || lower.contains("small")
        || lower.contains("qwen2.5-coder:7b")
        || lower.contains("llama3.2")
        || lower.contains("llama3.1:8b")
}

/// Robustly parse and repair tool arguments produced by small models.
/// Handles:
/// - Standard JSON objects: `{"path": "foo.txt"}`
/// - JSON wrapped in markdown code blocks: ````json {"path": "foo.txt"} ````
/// - JSON embedded in explanatory text: `Here are the arguments: {"path": "foo.txt"}`
/// - String-escaped inner JSON objects: `"{\"path\": \"foo.txt\"}"`
pub fn repair_tool_arguments(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Value::Object(serde_json::Map::new()));
    }

    // 1. Direct parse attempt
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }

    // 2. Escaped inner JSON string
    if let Ok(Value::String(inner_str)) = serde_json::from_str::<Value>(trimmed) {
        if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(inner_str.trim()) {
            return Some(v);
        }
    }

    // 3. Strip markdown fences (```json ... ``` or ``` ...)
    let cleaned = if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        let lines: Vec<&str> = trimmed.lines().collect();
        if lines.len() >= 2 {
            let start = 1;
            let end = if lines
                .last()
                .is_some_and(|l| l.trim().starts_with("```") || l.trim().starts_with("~~~"))
            {
                lines.len() - 1
            } else {
                lines.len()
            };
            lines[start..end].join("\n").trim().to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };

    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(&cleaned) {
        return Some(v);
    }

    // 4. Extract first outer JSON object {...}
    if let (Some(start), Some(end)) = (cleaned.find('{'), cleaned.rfind('}')) {
        if start < end {
            let slice = &cleaned[start..=end];
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(slice) {
                return Some(v);
            }
        }
    }

    None
}

/// Fallback parser: when a small model outputs tool calls into its text content
/// instead of OpenAI `tool_calls` structure.
pub fn extract_tool_calls_from_text(content: &str, valid_tools: &[&str]) -> Vec<ToolCall> {
    let mut results = Vec::new();

    // 1. Check for XML-style <tool_call>...</tool_call> or <tool>...</tool>
    let mut search_idx = 0;
    while let Some(open) = content[search_idx..].find("<tool_call>") {
        let start = search_idx + open + "<tool_call>".len();
        if let Some(close) = content[start..].find("</tool_call>") {
            let end = start + close;
            let snippet = &content[start..end].trim();
            if let Some(call) = parse_tool_json(snippet, valid_tools) {
                results.push(call);
            }
            search_idx = end + "</tool_call>".len();
        } else {
            break;
        }
    }

    if !results.is_empty() {
        return results;
    }

    // 2. Check for markdown json code blocks containing a tool call
    let mut block_idx = 0;
    while let Some(open) = content[block_idx..].find("```") {
        let start_line = block_idx + open;
        if let Some(line_end) = content[start_line..].find('\n') {
            let body_start = start_line + line_end + 1;
            if let Some(close) = content[body_start..].find("```") {
                let snippet = &content[body_start..body_start + close].trim();
                if let Some(call) = parse_tool_json(snippet, valid_tools) {
                    results.push(call);
                }
                block_idx = body_start + close + 3;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if !results.is_empty() {
        return results;
    }

    // 3. Check for inline `tool_name({...})` syntax
    for &tool in valid_tools {
        let needle = format!("{tool}(");
        let mut idx = 0;
        while let Some(pos) = content[idx..].find(&needle) {
            let start = idx + pos + needle.len() - 1; // pointing to '('
            if let Some(args_slice) = extract_balanced_parens(&content[start..]) {
                if let Some(args) = repair_tool_arguments(args_slice) {
                    let id = format!("call_extracted_{}_{}", tool, results.len() + 1);
                    results.push(ToolCall {
                        id,
                        kind: "function".to_string(),
                        function: FunctionCall {
                            name: tool.to_string(),
                            arguments: args.to_string(),
                        },
                    });
                }
                idx = start + args_slice.len() + 2;
            } else {
                idx = start + 1;
            }
        }
    }

    results
}

fn parse_tool_json(snippet: &str, valid_tools: &[&str]) -> Option<ToolCall> {
    let parsed: Value = serde_json::from_str(snippet).ok()?;
    let obj = parsed.as_object()?;

    let name = obj.get("name").and_then(Value::as_str)?;
    if !valid_tools.contains(&name) {
        return None;
    }

    let args_val = obj
        .get("arguments")
        .cloned()
        .unwrap_or(Value::Object(serde_json::Map::new()));
    let args_str = if args_val.is_string() {
        args_val.as_str().unwrap().to_string()
    } else {
        args_val.to_string()
    };

    let repaired = repair_tool_arguments(&args_str)?;
    let id = format!("call_parsed_{}", name);
    Some(ToolCall {
        id,
        kind: "function".to_string(),
        function: FunctionCall {
            name: name.to_string(),
            arguments: repaired.to_string(),
        },
    })
}

fn extract_balanced_parens(s: &str) -> Option<&str> {
    if !s.starts_with('(') {
        return None;
    }
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == '(' {
            if depth == 0 {
                start = i + 1;
            }
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[start..i]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_small_model_name() {
        assert!(is_small_model_name("ollama/qwen2.5-coder:7b"));
        assert!(is_small_model_name("llama3.2:3b"));
        assert!(is_small_model_name("groq/llama-3.1-8b-instant"));
        assert!(is_small_model_name("gemini-2.0-flash-lite"));
        assert!(!is_small_model_name("claude-3-5-sonnet-20241022"));
        assert!(!is_small_model_name("deepseek-r1"));
    }

    #[test]
    fn test_repair_clean_json() {
        let input = r#"{"path": "src/main.rs"}"#;
        let res = repair_tool_arguments(input).unwrap();
        assert_eq!(res["path"], "src/main.rs");
    }

    #[test]
    fn test_repair_markdown_code_fence() {
        let input = "```json\n{\"command\": \"cargo check\"}\n```";
        let res = repair_tool_arguments(input).unwrap();
        assert_eq!(res["command"], "cargo check");
    }

    #[test]
    fn test_repair_preamble_and_trailing_text() {
        let input = "I will edit the file now:\n{\"path\": \"test.txt\", \"target\": \"old\", \"replacement\": \"new\"}\nDone!";
        let res = repair_tool_arguments(input).unwrap();
        assert_eq!(res["path"], "test.txt");
        assert_eq!(res["target"], "old");
        assert_eq!(res["replacement"], "new");
    }

    #[test]
    fn test_repair_escaped_string() {
        let input = r#""{\"path\": \"foo.py\"}""#;
        let res = repair_tool_arguments(input).unwrap();
        assert_eq!(res["path"], "foo.py");
    }

    #[test]
    fn test_extract_tool_calls_xml_tag() {
        let content = "Let me read the file:\n<tool_call>{\"name\": \"read_file\", \"arguments\": {\"path\": \"README.md\"}}</tool_call>";
        let calls = extract_tool_calls_from_text(content, &["read_file", "write_file"]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "read_file");
        let args: Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
        assert_eq!(args["path"], "README.md");
    }

    #[test]
    fn test_extract_tool_calls_function_syntax() {
        let content = "Running command:\nshell({\"command\": \"git status\"})";
        let calls = extract_tool_calls_from_text(content, &["read_file", "shell"]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "shell");
        let args: Value = serde_json::from_str(&calls[0].function.arguments).unwrap();
        assert_eq!(args["command"], "git status");
    }
}
