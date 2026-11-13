//! Ordered fallback across (provider, model) targets.
//!
//! Free tiers run out or fail often, so a request walks the target list until
//! one answers. A rate-limited target cools down (honouring `Retry-After`) and
//! is skipped until the cooldown ends.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::client::{Client, ProviderConfig, ProviderError, StreamEvent};
use crate::types::{Completion, Message, ToolSpec};

#[derive(Debug, Clone)]
pub struct Target {
    pub provider: Arc<ProviderConfig>,
    pub model: String,
}

impl Target {
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider.name, self.model)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("every model failed:\n{}", .0.join("\n"))]
pub struct AllFailed(pub Vec<String>);

pub struct Router {
    client: Client,
    targets: Vec<Target>,
    cooldown_until: HashMap<String, Instant>,
    default_cooldown: Duration,
}

impl Router {
    pub fn new(targets: Vec<Target>) -> Self {
        Self {
            client: Client::new(),
            targets,
            cooldown_until: HashMap::new(),
            default_cooldown: Duration::from_secs(60),
        }
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// Keep only targets whose label contains `filter` (e.g. "groq" or "groq/openai/gpt-oss-120b").
    pub fn pin(&mut self, filter: &str) -> bool {
        let kept: Vec<Target> = self
            .targets
            .iter()
            .filter(|t| t.label().contains(filter))
            .cloned()
            .collect();
        if kept.is_empty() {
            return false;
        }
        self.targets = kept;
        true
    }

    pub fn status(&self) -> Vec<(String, Option<Duration>)> {
        let now = Instant::now();
        self.targets
            .iter()
            .map(|t| {
                let label = t.label();
                let left = self
                    .cooldown_until
                    .get(&label)
                    .and_then(|u| u.checked_duration_since(now));
                (label, left)
            })
            .collect()
    }

    pub async fn complete(
        &mut self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, AllFailed> {
        let mut failures = Vec::new();
        let now = Instant::now();
        let targets = self.targets.clone();
        for target in &targets {
            let label = target.label();
            if let Some(until) = self.cooldown_until.get(&label) {
                if *until > now {
                    let secs = until.duration_since(now).as_secs();
                    failures.push(format!("  {label}: cooling down ({secs}s left)"));
                    continue;
                }
            }
            match self
                .client
                .complete(&target.provider, &target.model, messages, tools, on_event)
                .await
            {
                Ok(c) => {
                    self.cooldown_until.remove(&label);
                    return Ok(c);
                }
                Err(err) => {
                    let cool = match &err {
                        ProviderError::RateLimited {
                            retry_after_secs, ..
                        } => Some(
                            retry_after_secs
                                .map(Duration::from_secs)
                                .unwrap_or(self.default_cooldown),
                        ),
                        // A bad key will not fix itself during this session.
                        ProviderError::Auth { .. } => Some(Duration::from_secs(3600)),
                        ProviderError::Server { .. } | ProviderError::Network(_) => {
                            Some(Duration::from_secs(15))
                        }
                        ProviderError::BadRequest { .. } | ProviderError::Protocol(_) => None,
                    };
                    if let Some(d) = cool {
                        self.cooldown_until
                            .insert(label.clone(), Instant::now() + d);
                    }
                    let reason = err.to_string();
                    on_event(StreamEvent::Fallback {
                        from: label.clone(),
                        reason: reason.clone(),
                    });
                    failures.push(format!("  {label}: {reason}"));
                }
            }
        }
        Err(AllFailed(failures))
    }
}
