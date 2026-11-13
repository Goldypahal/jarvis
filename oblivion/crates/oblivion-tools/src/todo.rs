//! `todo`: a task list the model keeps for multi-step work (SRS FR-17).
//! Each call replaces the whole list, so the model always sends its current view.

use std::sync::Mutex;

use oblivion_providers::ToolSpec;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Debug, Default)]
pub struct TodoList(Mutex<Vec<TodoItem>>);

impl TodoList {
    pub fn items(&self) -> Vec<TodoItem> {
        self.0.lock().unwrap().clone()
    }

    pub fn render(items: &[TodoItem]) -> String {
        if items.is_empty() {
            return "(no tasks)".into();
        }
        items
            .iter()
            .map(|t| {
                let mark = match t.status {
                    TodoStatus::Pending => "[ ]",
                    TodoStatus::InProgress => "[~]",
                    TodoStatus::Completed => "[x]",
                };
                format!("{mark} {}", t.content)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub(crate) fn spec() -> ToolSpec {
    ToolSpec {
        name: "todo".into(),
        description: "Keep a task list for work with 3 or more steps. Send the WHOLE list each time; \
            mark exactly one task in_progress while you work on it and completed as soon as it is done. \
            Skip this tool for simple one-step requests."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string"},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        }),
    }
}

pub(crate) fn update(list: &TodoList, args: &Value) -> Result<String, String> {
    let items: Vec<TodoItem> = serde_json::from_value(
        args.get("todos")
            .cloned()
            .ok_or("missing required argument 'todos'")?,
    )
    .map_err(|e| format!("todos must be a list of {{content, status}}: {e}"))?;
    let in_progress = items
        .iter()
        .filter(|t| t.status == TodoStatus::InProgress)
        .count();
    let rendered = TodoList::render(&items);
    *list.0.lock().unwrap() = items;
    let mut out = format!("Task list updated:\n{rendered}");
    if in_progress > 1 {
        out.push_str("\nNote: keep only one task in_progress at a time.");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_and_renders() {
        let l = TodoList::default();
        let out = update(
            &l,
            &json!({"todos":[
            {"content":"Read code","status":"completed"},
            {"content":"Fix bug","status":"in_progress"},
            {"content":"Run tests","status":"pending"}]}),
        )
        .unwrap();
        assert!(
            out.contains("[x] Read code")
                && out.contains("[~] Fix bug")
                && out.contains("[ ] Run tests")
        );
        assert_eq!(l.items().len(), 3);
        assert!(update(&l, &json!({"todos":[{"content":"x","status":"bogus"}]})).is_err());
    }
}
