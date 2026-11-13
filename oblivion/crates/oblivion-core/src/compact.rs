//! Keep the conversation inside the context budget (SRS FR-51/52).
//!
//! Free tiers have small request limits (Groq allows ~8K tokens per minute),
//! so the history is shrunk in two stages:
//! 1. old tool outputs are cut down to a short preview (cheap, no model call);
//! 2. if still too big, older turns are summarized by a model and replaced by
//!    the summary, keeping the most recent turns verbatim.

use oblivion_providers::{estimate_tokens, Message, Role, ToolSpec};

/// Tool outputs in the last this-many messages are never trimmed.
const KEEP_RECENT: usize = 6;
const TRIM_KEEP_CHARS: usize = 400;
pub const SUMMARY_MARKER: &str = "[Summary of the earlier conversation]";

pub const SUMMARY_PROMPT: &str = "You compress agent conversations. Write a concise summary \
(at most 250 words) of the transcript you are given, so the agent can continue the work. Keep: \
the user's goals and constraints, decisions made, files read or changed (with paths), commands \
run and their outcomes, errors still unresolved, and what remains to do. Plain text, no preamble.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    None,
    Trimmed,
    NeedsSummary,
}

pub fn over_budget(messages: &[Message], tools: &[ToolSpec], budget: usize) -> bool {
    estimate_tokens(messages, tools) > budget * 85 / 100
}

/// Stage 1: shorten old tool results in place. Returns what still needs doing.
pub fn trim_old_tool_output(messages: &mut [Message], tools: &[ToolSpec], budget: usize) -> Stage {
    if !over_budget(messages, tools, budget) {
        return Stage::None;
    }
    let cutoff = messages.len().saturating_sub(KEEP_RECENT);
    for m in messages[..cutoff].iter_mut() {
        if m.role != Role::Tool {
            continue;
        }
        if let Some(c) = &m.content {
            let n = c.chars().count();
            if n > TRIM_KEEP_CHARS + 100 {
                let head: String = c.chars().take(TRIM_KEEP_CHARS).collect();
                m.content = Some(format!(
                    "{head}\n[... {} characters of old tool output removed to save space]",
                    n - TRIM_KEEP_CHARS
                ));
            }
        }
    }
    if over_budget(messages, tools, budget) {
        Stage::NeedsSummary
    } else {
        Stage::Trimmed
    }
}

/// Where to split for stage 2: everything in `1..split` gets summarized and
/// `split..` is kept. The kept part never starts with a tool result (its
/// call would be gone) and fits in half the budget when possible.
pub fn split_point(messages: &[Message], tools: &[ToolSpec], budget: usize) -> Option<usize> {
    let n = messages.len();
    if n < 4 {
        return None;
    }
    let system = &messages[..1];
    // Latest safe start that still leaves at least two messages to summarize.
    let candidates: Vec<usize> = (3..n).filter(|&i| messages[i].role != Role::Tool).collect();
    // Prefer the earliest candidate whose tail fits; that keeps the most context.
    for &i in &candidates {
        let mut probe = system.to_vec();
        probe.extend_from_slice(&messages[i..]);
        if estimate_tokens(&probe, tools) <= budget / 2 {
            return Some(i);
        }
    }
    // Nothing fits half the budget: keep only the newest safe start.
    candidates.last().copied()
}

/// Plain-text transcript of `messages` for the summarizer, capped to `max_chars`.
pub fn transcript(messages: &[Message], max_chars: usize) -> String {
    let mut out = String::new();
    for m in messages {
        let who = match m.role {
            Role::System => continue,
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::Tool => "TOOL RESULT",
        };
        if let Some(c) = &m.content {
            if !c.is_empty() {
                let c: String = c.chars().take(1500).collect();
                out.push_str(&format!("{who}: {c}\n"));
            }
        }
        for call in &m.tool_calls {
            let args: String = call.function.arguments.chars().take(300).collect();
            out.push_str(&format!(
                "ASSISTANT called {}({args})\n",
                call.function.name
            ));
        }
    }
    let total = out.chars().count();
    if total > max_chars {
        let half = max_chars / 2;
        let head: String = out.chars().take(half).collect();
        let tail: String = out.chars().skip(total - half).collect();
        out = format!("{head}\n[...]\n{tail}");
    }
    out
}

/// Replace `messages[1..split]` with `summary`, keeping the message order valid.
pub fn apply_summary(messages: &mut Vec<Message>, split: usize, summary: &str) {
    let note = format!("{SUMMARY_MARKER}\n{}", summary.trim());
    let mut tail = messages.split_off(split);
    messages.truncate(1);
    if let Some(first) = tail.first_mut() {
        if first.role == Role::User {
            let text = first.content.take().unwrap_or_default();
            first.content = Some(format!("{note}\n\n{text}"));
            messages.append(&mut tail);
            return;
        }
    }
    messages.push(Message::user(note));
    messages.append(&mut tail);
}

#[cfg(test)]
mod tests {
    use super::*;
    use oblivion_providers::{FunctionCall, ToolCall};

    fn call(id: &str) -> Message {
        Message::assistant(
            None,
            vec![ToolCall {
                id: id.into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: "{}".into(),
                },
            }],
        )
    }

    fn convo() -> Vec<Message> {
        let big = "x".repeat(8000);
        vec![
            Message::system("sys"),
            Message::user("task one"),
            call("a"),
            Message::tool_result("a", big.clone()),
            Message::assistant(Some("done one".into()), vec![]),
            Message::user("task two"),
            call("b"),
            Message::tool_result("b", big.clone()),
            call("c"),
            Message::tool_result("c", big),
            Message::assistant(Some("done two".into()), vec![]),
        ]
    }

    #[test]
    fn trims_only_old_tool_output() {
        let mut m = convo();
        let stage = trim_old_tool_output(&mut m, &[], 3000);
        assert_ne!(stage, Stage::None);
        assert!(m[3]
            .content
            .as_ref()
            .unwrap()
            .contains("removed to save space"));
        // Within the last 6 messages: untouched.
        assert_eq!(m[9].content.as_ref().unwrap().len(), 8000);
    }

    #[test]
    fn split_never_orphans_tool_results() {
        let m = convo();
        let i = split_point(&m, &[], 2000).unwrap();
        assert_ne!(m[i].role, Role::Tool);
        assert!(i >= 3);
    }

    #[test]
    fn summary_merges_into_user_message_or_is_inserted() {
        let mut m = convo();
        apply_summary(&mut m, 5, "did task one");
        assert_eq!(m[0].role, Role::System);
        assert_eq!(m[1].role, Role::User);
        let c = m[1].content.as_ref().unwrap();
        assert!(
            c.starts_with(SUMMARY_MARKER) && c.contains("did task one") && c.ends_with("task two")
        );

        let mut m = convo();
        apply_summary(&mut m, 6, "s");
        assert_eq!(m[1].role, Role::User);
        assert_eq!(m[2].tool_calls[0].id, "b");
    }

    #[test]
    fn transcript_is_capped() {
        let t = transcript(&convo(), 500);
        assert!(t.chars().count() <= 520);
        assert!(t.starts_with("USER: task one"));
    }
}
