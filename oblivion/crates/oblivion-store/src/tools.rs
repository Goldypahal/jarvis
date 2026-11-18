//! Tools for interacting with Oblivion's long-term memory: `remember`, `recall`, `forget`.
//!
//! Provides the [`MemoryTools`] tool provider, which implements [`ToolProvider`]
//! from `oblivion-tools` so the agent can store, search, and forget persistent
//! memories across sessions.

use std::sync::Arc;

use oblivion_providers::ToolSpec;
use oblivion_tools::{Risk, ToolFuture, ToolProvider};
use serde_json::{json, Value};

use crate::{MemoryFilter, NewMemory, Store};

pub struct MemoryTools {
    store: Arc<Store>,
    session_id: std::sync::Mutex<Option<String>>,
    project: std::sync::Mutex<Option<String>>,
}

impl MemoryTools {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            session_id: std::sync::Mutex::new(None),
            project: std::sync::Mutex::new(None),
        }
    }

    pub fn with_session(store: Arc<Store>, session_id: impl Into<String>) -> Self {
        Self {
            store,
            session_id: std::sync::Mutex::new(Some(session_id.into())),
            project: std::sync::Mutex::new(None),
        }
    }

    pub fn with_project(store: Arc<Store>, project: impl Into<String>) -> Self {
        Self {
            store,
            session_id: std::sync::Mutex::new(None),
            project: std::sync::Mutex::new(Some(project.into())),
        }
    }

    pub fn set_session(&self, session_id: impl Into<String>) {
        if let Ok(mut guard) = self.session_id.lock() {
            *guard = Some(session_id.into());
        }
    }

    pub fn set_project(&self, project: impl Into<String>) {
        if let Ok(mut guard) = self.project.lock() {
            *guard = Some(project.into());
        }
    }

    async fn exec_remember(&self, args: &Value) -> Result<String, String> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| "remember requires a 'text' parameter".to_string())?
            .trim();
        if text.is_empty() {
            return Err("memory text cannot be empty".into());
        }

        let mem_type = args.get("type").and_then(Value::as_str).unwrap_or("fact");
        let importance = args.get("importance").and_then(Value::as_u64).unwrap_or(3) as u32;

        let mut new_mem = NewMemory::new(mem_type, text, importance);
        if let Ok(guard) = self.session_id.lock() {
            if let Some(ref s) = *guard {
                new_mem = new_mem.with_source_session(s);
            }
        }
        if let Some(p) = args.get("project").and_then(Value::as_str) {
            new_mem = new_mem.with_project(p);
        } else if let Ok(guard) = self.project.lock() {
            if let Some(ref p) = *guard {
                if mem_type == "project_fact" || mem_type == "recipe" {
                    new_mem = new_mem.with_project(p);
                }
            }
        }

        let created = self
            .store
            .add_memory(&new_mem)
            .map_err(|e| format!("failed to save memory: {e}"))?;

        if let Some(embedder) = self.store.embedder() {
            if let Ok(vec) = embedder.embed(&created.text).await {
                let _ = self
                    .store
                    .save_memory_embedding(created.id, embedder.model_name(), &vec);
            }
        }

        Ok(format!(
            "Remembered [ID: {}] ({}, importance {}): {}",
            created.id, created.mem_type, created.importance, created.text
        ))
    }

    async fn exec_recall(&self, args: &Value) -> Result<String, String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| "recall requires a 'query' parameter".to_string())?
            .trim();

        let mem_type = args.get("type").and_then(Value::as_str).map(String::from);
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 20) as usize;

        let project = args
            .get("project")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| self.project.lock().ok().and_then(|g| g.clone()));

        let filter = MemoryFilter {
            mem_type,
            project,
            include_global: true,
            ..Default::default()
        };

        let embedding = if let Some(embedder) = self.store.embedder() {
            embedder.embed(query).await.ok()
        } else {
            None
        };

        let results = self
            .store
            .search_memories_hybrid(query, embedding.as_deref(), Some(&filter), limit)
            .map_err(|e| format!("failed to recall memories: {e}"))?;

        if results.is_empty() {
            return Ok(format!("No memories found matching '{query}'."));
        }

        // Touch all recalled memories so recency is tracked
        for m in &results {
            let _ = self.store.touch_memory(m.id);
        }

        let mut out = format!("Found {} relevant memories:\n", results.len());
        for m in results {
            out.push_str(&format!(
                "- [ID: {}] ({}, importance {}): {}\n",
                m.id, m.mem_type, m.importance, m.text
            ));
        }
        Ok(out.trim_end().to_string())
    }

    fn exec_forget(&self, args: &Value) -> Result<String, String> {
        let id_opt = args.get("id").and_then(Value::as_i64);
        let query_opt = args.get("query").and_then(Value::as_str).map(str::trim);

        if id_opt.is_none() && query_opt.is_none() {
            return Err("forget requires either an 'id' or a 'query' parameter".into());
        }

        if let Some(id) = id_opt {
            let existing = self
                .store
                .get_memory(id)
                .map_err(|e| format!("failed to read memory: {e}"))?;
            let Some(mem) = existing else {
                return Ok(format!("Memory [ID: {id}] not found."));
            };
            let deleted = self
                .store
                .delete_memory(id)
                .map_err(|e| format!("failed to delete memory: {e}"))?;
            if deleted {
                Ok(format!("Forgot memory [ID: {id}]: {}", mem.text))
            } else {
                Ok(format!("Memory [ID: {id}] not found."))
            }
        } else {
            let query = query_opt.unwrap();
            if query.is_empty() {
                return Err("forget query cannot be empty".into());
            }
            let matching = self
                .store
                .search_memories(query, None, 50)
                .map_err(|e| format!("failed to search memories: {e}"))?;
            if matching.is_empty() {
                return Ok(format!("No memories found matching '{query}' to forget."));
            }
            let mut count = 0;
            for m in matching {
                if self.store.delete_memory(m.id).unwrap_or(false) {
                    count += 1;
                }
            }
            Ok(format!("Forgot {count} memories matching '{query}'."))
        }
    }
}

impl ToolProvider for MemoryTools {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "remember".into(),
                description: "Save a fact, user preference, recipe, or past outcome into long-term memory across sessions.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The exact fact, preference, or lesson to remember."
                        },
                        "type": {
                            "type": "string",
                            "enum": ["user_pref", "project_fact", "fact", "recipe", "past_outcome"],
                            "description": "Category of memory (defaults to 'fact')."
                        },
                        "importance": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 5,
                            "description": "Importance rating from 1 (low) to 5 (critical, e.g. core preference). Defaults to 3."
                        },
                        "project": {
                            "type": "string",
                            "description": "Optional project identifier to scope this memory to."
                        }
                    },
                    "required": ["text"]
                }),
            },
            ToolSpec {
                name: "recall".into(),
                description: "Search long-term memory for relevant user preferences, project facts, recipes, or past outcomes.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search keywords or topic to find in memory."
                        },
                        "type": {
                            "type": "string",
                            "enum": ["user_pref", "project_fact", "fact", "recipe", "past_outcome"],
                            "description": "Optional category filter."
                        },
                        "limit": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 20,
                            "description": "Maximum number of memories to return (defaults to 5)."
                        },
                        "project": {
                            "type": "string",
                            "description": "Optional project identifier to filter memories by."
                        }
                    },
                    "required": ["query"]
                }),
            },
            ToolSpec {
                name: "forget".into(),
                description: "Delete a specific memory by its ID, or forget memories matching a search query/topic.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "integer",
                            "description": "The numeric ID of a specific memory to delete."
                        },
                        "query": {
                            "type": "string",
                            "description": "Keywords or topic: deletes memories matching this query."
                        }
                    }
                }),
            },
        ]
    }

    fn handles(&self, name: &str) -> bool {
        matches!(name, "remember" | "recall" | "forget")
    }

    fn risk(&self, name: &str, args: &Value) -> Risk {
        match name {
            "recall" => Risk::Safe,
            "remember" => {
                let text = args.get("text").and_then(Value::as_str).unwrap_or("");
                let preview: String = text.chars().take(60).collect();
                Risk::Ask(format!("remember: {preview}"))
            }
            "forget" => {
                let target = if let Some(id) = args.get("id").and_then(Value::as_i64) {
                    format!("memory id {id}")
                } else if let Some(q) = args.get("query").and_then(Value::as_str) {
                    format!("memories matching '{q}'")
                } else {
                    "memory".to_string()
                };
                Risk::Ask(format!("forget {target}"))
            }
            _ => Risk::Safe,
        }
    }

    fn subject(&self, name: &str, args: &Value) -> String {
        match name {
            "recall" => args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            "remember" => args
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            "forget" => {
                if let Some(id) = args.get("id").and_then(Value::as_i64) {
                    format!("id:{id}")
                } else {
                    args.get("query")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                }
            }
            _ => String::new(),
        }
    }

    fn execute<'a>(&'a self, name: &'a str, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            match name {
                "remember" => self.exec_remember(args).await,
                "recall" => self.exec_recall(args).await,
                "forget" => self.exec_forget(args),
                other => Err(format!("unknown memory tool '{other}'")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oblivion_core::rules::{Rules, Verdict};

    #[test]
    fn specs_and_handles() {
        let store = Arc::new(Store::in_memory().unwrap());
        let tools = MemoryTools::new(store);

        assert!(tools.handles("remember"));
        assert!(tools.handles("recall"));
        assert!(tools.handles("forget"));
        assert!(!tools.handles("shell"));

        let specs = tools.specs();
        assert_eq!(specs.len(), 3);
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"recall"));
        assert!(names.contains(&"forget"));
    }

    #[test]
    fn risk_and_subject() {
        let store = Arc::new(Store::in_memory().unwrap());
        let tools = MemoryTools::new(store);

        // recall is Safe
        let recall_args = json!({ "query": "rust async" });
        assert_eq!(tools.risk("recall", &recall_args), Risk::Safe);
        assert_eq!(tools.subject("recall", &recall_args), "rust async");

        // remember asks
        let remember_args =
            json!({ "text": "user likes light theme", "type": "user_pref", "importance": 4 });
        assert_eq!(
            tools.risk("remember", &remember_args),
            Risk::Ask("remember: user likes light theme".into())
        );
        assert_eq!(
            tools.subject("remember", &remember_args),
            "user likes light theme"
        );

        // forget asks (by id and by query)
        let forget_id_args = json!({ "id": 42 });
        assert_eq!(
            tools.risk("forget", &forget_id_args),
            Risk::Ask("forget memory id 42".into())
        );
        assert_eq!(tools.subject("forget", &forget_id_args), "id:42");

        let forget_query_args = json!({ "query": "light theme" });
        assert_eq!(
            tools.risk("forget", &forget_query_args),
            Risk::Ask("forget memories matching 'light theme'".into())
        );
        assert_eq!(tools.subject("forget", &forget_query_args), "light theme");
    }

    #[tokio::test]
    async fn remember_recall_forget_lifecycle() {
        let store = Arc::new(Store::in_memory().unwrap());
        let tools = MemoryTools::with_session(store, "test-sess");

        // 1. Remember facts
        let r1 = tools
            .execute(
                "remember",
                &json!({
                    "text": "User prefers concise answers with zero boilerplate",
                    "type": "user_pref",
                    "importance": 5
                }),
            )
            .await
            .unwrap();
        assert!(r1.contains("Remembered [ID: 1]"));
        assert!(r1.contains("user_pref"));

        let r2 = tools
            .execute(
                "remember",
                &json!({
                    "text": "Project uses SQLite FTS5 for full-text search",
                    "type": "project_fact",
                    "importance": 4
                }),
            )
            .await
            .unwrap();
        assert!(r2.contains("Remembered [ID: 2]"));

        // 2. Recall
        let recall_res = tools
            .execute("recall", &json!({ "query": "concise" }))
            .await
            .unwrap();
        assert!(recall_res.contains("Found 1 relevant memories:"));
        assert!(recall_res.contains("User prefers concise answers"));

        let recall_fts = tools
            .execute("recall", &json!({ "query": "FTS5" }))
            .await
            .unwrap();
        assert!(recall_fts.contains("Project uses SQLite FTS5"));

        let recall_missing = tools
            .execute("recall", &json!({ "query": "nonexistent" }))
            .await
            .unwrap();
        assert!(recall_missing.contains("No memories found matching 'nonexistent'."));

        // 3. Forget by ID
        let forget_id_res = tools.execute("forget", &json!({ "id": 1 })).await.unwrap();
        assert!(forget_id_res.contains("Forgot memory [ID: 1]"));

        // Recall again - should be gone
        let recall_after_forget = tools
            .execute("recall", &json!({ "query": "concise" }))
            .await
            .unwrap();
        assert!(recall_after_forget.contains("No memories found matching 'concise'."));

        // 4. Forget by query
        let forget_query_res = tools
            .execute("forget", &json!({ "query": "SQLite" }))
            .await
            .unwrap();
        assert!(forget_query_res.contains("Forgot 1 memories matching 'SQLite'."));

        let recall_after_query_forget = tools
            .execute("recall", &json!({ "query": "FTS5" }))
            .await
            .unwrap();
        assert!(recall_after_query_forget.contains("No memories found matching 'FTS5'."));
    }

    #[test]
    fn permission_rules_match_memory_tools() {
        let store = Arc::new(Store::in_memory().unwrap());
        let tools = MemoryTools::new(store);

        let allow = vec![
            "recall(*)".to_string(),
            "remember(user pref: *)".to_string(),
        ];
        let deny = vec!["forget(*)".to_string(), "remember(*secret*)".to_string()];
        let rules = Rules::new(&allow, &deny).unwrap();

        // 1. recall is allowed by rule
        let recall_subject = tools.subject("recall", &json!({ "query": "anything" }));
        assert_eq!(
            rules.check("recall", &recall_subject),
            Verdict::Allow("recall(*)".into())
        );

        // 2. remember with matching prefix is allowed
        let rem_good = json!({ "text": "user pref: prefers vim keybindings" });
        let rem_good_subj = tools.subject("remember", &rem_good);
        assert_eq!(
            rules.check("remember", &rem_good_subj),
            Verdict::Allow("remember(user pref: *)".into())
        );

        // 3. remember containing 'secret' is denied
        let rem_secret = json!({ "text": "user secret password" });
        let rem_secret_subj = tools.subject("remember", &rem_secret);
        assert_eq!(
            rules.check("remember", &rem_secret_subj),
            Verdict::Deny("remember(*secret*)".into())
        );

        // 4. remember normal fact has NoRule (so it falls back to Ask in agent)
        let rem_fact = json!({ "text": "server is running on port 8080" });
        let rem_fact_subj = tools.subject("remember", &rem_fact);
        assert_eq!(rules.check("remember", &rem_fact_subj), Verdict::NoRule);

        // 5. forget is always denied
        let forget_subj = tools.subject("forget", &json!({ "id": 1 }));
        assert_eq!(
            rules.check("forget", &forget_subj),
            Verdict::Deny("forget(*)".into())
        );
    }

    #[tokio::test]
    async fn toolbox_integration_with_memory_tools() {
        let store = Arc::new(Store::in_memory().unwrap());
        let memory_tools = Arc::new(MemoryTools::with_session(store.clone(), "sess-100"));
        let dir = tempfile::tempdir().unwrap();
        let mut tb = oblivion_tools::ToolBox::new(
            dir.path(),
            oblivion_tools::Shell::Sh,
            dir.path().join(".snap"),
        );
        tb.add_provider(memory_tools);

        // 1. Tool specs contain memory tools
        let specs = tb.specs();
        let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"recall"));
        assert!(names.contains(&"forget"));

        // 2. Risk check
        let rem_args = json!({ "text": "prefers tabs over spaces" });
        assert!(matches!(tb.risk("remember", &rem_args), Risk::Ask(_)));
        assert_eq!(tb.risk("recall", &json!({ "query": "tabs" })), Risk::Safe);

        // 3. Execution via ToolBox
        let out = tb.execute("remember", &rem_args).await;
        assert!(!out.is_error);
        assert!(out.text.contains("Remembered [ID: 1]"));

        let recall_out = tb.execute("recall", &json!({ "query": "tabs" })).await;
        assert!(!recall_out.is_error);
        assert!(recall_out.text.contains("prefers tabs over spaces"));

        let forget_out = tb.execute("forget", &json!({ "id": 1 })).await;
        assert!(!forget_out.is_error);
        assert!(forget_out.text.contains("Forgot memory [ID: 1]"));

        let recall_after = tb.execute("recall", &json!({ "query": "tabs" })).await;
        assert!(!recall_after.is_error);
        assert!(recall_after
            .text
            .contains("No memories found matching 'tabs'."));
    }
}
