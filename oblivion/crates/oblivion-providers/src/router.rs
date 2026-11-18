//! Ordered fallback across (provider, model) targets (SRS 3.4).
//!
//! Free tiers run out or fail often, so a request walks the target list until
//! one answers. A target is skipped when it is cooling down after an error,
//! when the request is bigger than its free-tier limit, or when it has used
//! its daily request allowance (tracked by an optional [`UsageLedger`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::client::{Client, ProviderConfig, ProviderError, StreamEvent};
use crate::types::{estimate_tokens, Completion, Message, ToolSpec, Usage};

#[derive(Debug, Clone)]
pub struct Target {
    pub provider: Arc<ProviderConfig>,
    pub model: String,
    /// Largest request (in estimated tokens) this model accepts on its free tier.
    pub max_input_tokens: Option<u32>,
    /// Free-tier requests per day; the target is skipped once it is used up.
    pub requests_per_day: Option<u32>,
    /// Date limits were last verified (YYYY-MM-DD).
    pub checked: Option<String>,
}

impl Target {
    pub fn new(provider: Arc<ProviderConfig>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            max_input_tokens: None,
            requests_per_day: None,
            checked: None,
        }
    }

    pub fn label(&self) -> String {
        format!("{}/{}", self.provider.name, self.model)
    }
}

/// Persistent record of how many requests each target made today.
pub trait UsageLedger: Send + Sync {
    fn requests_today(&self, label: &str) -> u64;
    fn record(&self, label: &str, usage: Option<&Usage>);
}

#[derive(Debug, thiserror::Error)]
#[error("every model failed:\n{}", .0.join("\n"))]
pub struct AllFailed(pub Vec<String>);

/// One row of `/models` output.
#[derive(Debug, Clone)]
pub struct TargetStatus {
    pub label: String,
    pub cooldown: Option<Duration>,
    pub used_today: Option<u64>,
    pub requests_per_day: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub checked: Option<String>,
}

/// Cooldowns, shared by every router made with [`Router::share`] so parallel
/// sub-agents do not all hammer a provider that just said "slow down".
#[derive(Default)]
struct Health {
    cooldown_until: HashMap<String, Instant>,
    /// Targets whose current cooldown came from a rate limit (worth waiting for).
    rate_limited: HashMap<String, Instant>,
}

pub struct Router {
    client: Client,
    targets: Vec<Target>,
    health: Arc<Mutex<Health>>,
    default_cooldown: Duration,
    /// Longest single wait when every model is rate-limited; longer waits fail the request.
    max_wait: Duration,
    ledger: Option<Arc<dyn UsageLedger>>,
}

impl Router {
    pub fn new(targets: Vec<Target>) -> Self {
        Self {
            client: Client::new(),
            targets,
            health: Arc::new(Mutex::new(Health::default())),
            default_cooldown: Duration::from_secs(60),
            max_wait: Duration::from_secs(65),
            ledger: None,
        }
    }

    pub fn with_max_wait(mut self, d: Duration) -> Self {
        self.max_wait = d;
        self
    }

    pub fn with_ledger(mut self, ledger: Arc<dyn UsageLedger>) -> Self {
        self.ledger = Some(ledger);
        self
    }

    /// A second router over the same targets that shares cooldowns and the
    /// usage ledger (for sub-agents and background summaries).
    pub fn share(&self) -> Router {
        Router {
            client: self.client.clone(),
            targets: self.targets.clone(),
            health: self.health.clone(),
            default_cooldown: self.default_cooldown,
            max_wait: self.max_wait,
            ledger: self.ledger.clone(),
        }
    }

    /// Move targets whose label contains `filter` to the front, keeping the
    /// rest as fallbacks. An empty filter or no match leaves the order alone.
    pub fn preferring(mut self, filter: &str) -> Router {
        if !filter.is_empty() {
            let (mut first, rest): (Vec<Target>, Vec<Target>) = self
                .targets
                .into_iter()
                .partition(|t| t.label().contains(filter));
            first.extend(rest);
            self.targets = first;
        }
        self
    }

    pub fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// The largest request any target accepts; `None` if some target has no limit.
    pub fn largest_input_budget(&self) -> Option<u32> {
        let mut best = 0;
        for t in &self.targets {
            best = best.max(t.max_input_tokens?);
        }
        (best > 0).then_some(best)
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

    pub fn status(&self) -> Vec<TargetStatus> {
        let now = Instant::now();
        self.targets
            .iter()
            .map(|t| {
                let label = t.label();
                TargetStatus {
                    cooldown: self
                        .health
                        .lock()
                        .unwrap()
                        .cooldown_until
                        .get(&label)
                        .and_then(|u| u.checked_duration_since(now)),
                    used_today: self.ledger.as_ref().map(|l| l.requests_today(&label)),
                    requests_per_day: t.requests_per_day,
                    max_input_tokens: t.max_input_tokens,
                    checked: t.checked.clone(),
                    label,
                }
            })
            .collect()
    }

    /// Try every target; if all of them are only briefly rate-limited (free
    /// tiers have small per-minute limits), wait as asked and try again.
    pub async fn complete(
        &mut self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, AllFailed> {
        const MAX_ROUNDS: usize = 6;
        let mut round = 0;
        loop {
            round += 1;
            match self.try_once(messages, tools, on_event).await {
                Ok(c) => return Ok(c),
                Err(failures) => {
                    let now = Instant::now();
                    // Only worth waiting if some target is held back by a rate limit alone.
                    let wait = self
                        .health
                        .lock()
                        .unwrap()
                        .rate_limited
                        .iter()
                        .filter(|(label, _)| self.targets.iter().any(|t| &t.label() == *label))
                        .map(|(_, until)| until.saturating_duration_since(now))
                        .min();
                    match wait {
                        Some(w) if round < MAX_ROUNDS && w <= self.max_wait => {
                            // Round up and add a moment so the provider's window has really passed.
                            let secs = w.as_secs() + 1;
                            on_event(StreamEvent::Waiting { seconds: secs });
                            tokio::time::sleep(Duration::from_secs(secs)).await;
                        }
                        _ => return Err(failures),
                    }
                }
            }
        }
    }

    async fn try_once(
        &mut self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<Completion, AllFailed> {
        let mut failures = Vec::new();
        let now = Instant::now();
        let size = estimate_tokens(messages, tools);
        let targets = self.targets.clone();
        for target in &targets {
            let label = target.label();
            let cooling = self
                .health
                .lock()
                .unwrap()
                .cooldown_until
                .get(&label)
                .copied();
            if let Some(until) = cooling {
                if until > now {
                    let secs = until.duration_since(now).as_secs();
                    failures.push(format!("  {label}: cooling down ({secs}s left)"));
                    continue;
                }
            }
            if let Some(max) = target.max_input_tokens {
                if size > max as usize {
                    failures.push(format!(
                        "  {label}: request too large (~{size} tokens, limit {max})"
                    ));
                    continue;
                }
            }
            if let (Some(limit), Some(ledger)) = (target.requests_per_day, &self.ledger) {
                let used = ledger.requests_today(&label);
                if used >= limit as u64 {
                    failures.push(format!("  {label}: daily limit reached ({used}/{limit})"));
                    continue;
                }
            }
            match self
                .client
                .complete(&target.provider, &target.model, messages, tools, on_event)
                .await
            {
                Ok(c) => {
                    {
                        let mut h = self.health.lock().unwrap();
                        h.cooldown_until.remove(&label);
                        h.rate_limited.remove(&label);
                    }
                    if let Some(l) = &self.ledger {
                        l.record(&label, c.usage.as_ref());
                    }
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
                    {
                        let mut h = self.health.lock().unwrap();
                        if let Some(d) = cool {
                            h.cooldown_until.insert(label.clone(), Instant::now() + d);
                        }
                        if matches!(err, ProviderError::RateLimited { .. }) {
                            let until = Instant::now() + cool.unwrap_or_default();
                            h.rate_limited.insert(label.clone(), until);
                        } else {
                            h.rate_limited.remove(&label);
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemLedger(Mutex<HashMap<String, u64>>);
    impl UsageLedger for MemLedger {
        fn requests_today(&self, label: &str) -> u64 {
            *self.0.lock().unwrap().get(label).unwrap_or(&0)
        }
        fn record(&self, label: &str, _u: Option<&Usage>) {
            *self.0.lock().unwrap().entry(label.into()).or_default() += 1;
        }
    }

    fn target(name: &str, max_in: Option<u32>, rpd: Option<u32>) -> Target {
        let p = Arc::new(ProviderConfig {
            name: name.into(),
            // Port 9 (discard) on localhost: connection refused, so a call fails fast.
            base_url: "http://127.0.0.1:9".into(),
            api_key: None,
            headers: vec![],
            stream_usage: false,
        });
        Target {
            max_input_tokens: max_in,
            requests_per_day: rpd,
            ..Target::new(p, "m")
        }
    }

    #[tokio::test]
    async fn skips_oversize_and_exhausted_targets_without_calling_them() {
        let ledger = Arc::new(MemLedger::default());
        ledger.0.lock().unwrap().insert("used/m".into(), 5);
        let mut r = Router::new(vec![
            target("small", Some(10), None),
            target("used", None, Some(5)),
        ])
        .with_ledger(ledger);
        let msgs = vec![Message::user("x".repeat(400))];
        let mut sink = |_e: StreamEvent| {};
        let err = r
            .complete(&msgs, &[], &mut sink)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("small/m: request too large"), "{err}");
        assert!(err.contains("used/m: daily limit reached (5/5)"), "{err}");
    }

    #[test]
    fn preferring_reorders_and_share_keeps_cooldowns() {
        let r = Router::new(vec![
            target("groq", None, None),
            target("ollama", None, None),
            target("or", None, None),
        ]);
        let p = r.share().preferring("ollama");
        let labels: Vec<String> = p.targets().iter().map(|t| t.label()).collect();
        assert_eq!(labels, ["ollama/m", "groq/m", "or/m"]);
        r.health
            .lock()
            .unwrap()
            .cooldown_until
            .insert("groq/m".into(), Instant::now() + Duration::from_secs(30));
        assert!(p
            .status()
            .iter()
            .any(|s| s.label == "groq/m" && s.cooldown.is_some()));
        assert_eq!(r.share().preferring("").targets()[0].label(), "groq/m");
    }

    #[test]
    fn largest_budget() {
        let r = Router::new(vec![
            target("a", Some(8000), None),
            target("b", Some(100_000), None),
        ]);
        assert_eq!(r.largest_input_budget(), Some(100_000));
        let r = Router::new(vec![target("a", Some(8000), None), target("b", None, None)]);
        assert_eq!(r.largest_input_budget(), None);
    }
}
