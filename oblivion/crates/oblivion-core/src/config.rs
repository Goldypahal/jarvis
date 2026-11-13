//! `oblivion.toml`: providers, models and agent settings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use oblivion_providers::{ProviderConfig, Target};
use serde::{Deserialize, Serialize};

/// Written by `oblivion --init`. Free model rosters change monthly, so the
/// file tells the user to check model IDs on each provider's site.
pub const DEFAULT_CONFIG: &str = r#"# Oblivion configuration
#
# Providers are tried top to bottom; within a provider, models are tried in order.
# If one fails or hits its free-tier limit, Oblivion moves to the next automatically.
# Free model IDs change often: check each provider's model list and edit the IDs below.
# API keys are read from environment variables (never put keys in this file).

[agent]
max_steps = 50          # tool-call rounds per request before stopping
approval = "ask"        # ask | auto-edit | yolo
# shell = "powershell"  # powershell | cmd | bash | sh (default: powershell on Windows, bash elsewhere)

# OpenRouter: 20 req/min, 50 req/day on ":free" models (1,000/day after a one-time $10 credit).
# Keys: https://openrouter.ai/keys
[[providers]]
name = "openrouter"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
models = ["qwen/qwen3-coder:free", "openai/gpt-oss-120b:free"]
headers = { "HTTP-Referer" = "https://github.com/Goldypahal/jarvis", "X-Title" = "Oblivion" }

# Groq: fast; per model about 30 req/min and 1,000 req/day on the free plan.
# Keys: https://console.groq.com/keys
[[providers]]
name = "groq"
base_url = "https://api.groq.com/openai/v1"
api_key_env = "GROQ_API_KEY"
models = ["openai/gpt-oss-120b", "openai/gpt-oss-20b"]

# NVIDIA API catalog: about 40 req/min for prototyping. Keys: https://build.nvidia.com
[[providers]]
name = "nvidia"
base_url = "https://integrate.api.nvidia.com/v1"
api_key_env = "NVIDIA_API_KEY"
models = ["openai/gpt-oss-120b"]

# Google Gemini (OpenAI-compatible endpoint). Free tier prompts may be used for training:
# do not use it for private files. Keys: https://aistudio.google.com/apikey
[[providers]]
name = "gemini"
base_url = "https://generativelanguage.googleapis.com/v1beta/openai"
api_key_env = "GEMINI_API_KEY"
models = ["gemini-2.5-flash"]

# Mistral: $10/month free API credit. Keys: https://console.mistral.ai
[[providers]]
name = "mistral"
base_url = "https://api.mistral.ai/v1"
api_key_env = "MISTRAL_API_KEY"
models = ["mistral-small-latest"]

# Local models through Ollama (offline, private). Install from https://ollama.com,
# run `ollama pull qwen3.5:9b`, then set enabled = true.
[[providers]]
name = "ollama"
base_url = "http://localhost:11434/v1"
models = ["qwen3.5:9b"]
enabled = false
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalMode {
    /// Ask before every file change and command.
    #[default]
    Ask,
    /// Edit files inside the workspace freely; ask for commands and anything outside.
    AutoEdit,
    /// Never ask. Use only in a throwaway folder.
    Yolo,
}

impl ApprovalMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ask" => Some(Self::Ask),
            "auto-edit" | "auto_edit" => Some(Self::AutoEdit),
            "yolo" => Some(Self::Yolo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
    #[serde(default)]
    pub approval: ApprovalMode,
    #[serde(default)]
    pub shell: Option<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
            approval: ApprovalMode::Ask,
            shell: None,
        }
    }
}

fn default_max_steps() -> usize {
    50
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub name: String,
    pub base_url: String,
    /// Environment variable holding the key. Omit for keyless local servers.
    #[serde(default)]
    pub api_key_env: Option<String>,
    pub models: Vec<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub stream_usage: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
}

/// Why a provider was left out, for `oblivion --models`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

impl Config {
    pub fn parse(text: &str) -> Result<Self> {
        toml::from_str(text).context("oblivion.toml is not valid")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "cannot read {}. Run `oblivion --init` to create it.",
                path.display()
            )
        })?;
        Self::parse(&text)
    }

    /// Build the router's target list from enabled providers that have keys.
    pub fn targets(&self, env: &dyn Fn(&str) -> Option<String>) -> (Vec<Target>, Vec<Skipped>) {
        let mut targets = Vec::new();
        let mut skipped = Vec::new();
        for p in &self.providers {
            if !p.enabled {
                skipped.push(Skipped {
                    name: p.name.clone(),
                    reason: "disabled in config".into(),
                });
                continue;
            }
            let api_key = match &p.api_key_env {
                Some(var) => match env(var).filter(|v| !v.trim().is_empty()) {
                    Some(k) => Some(k.trim().to_string()),
                    None => {
                        skipped.push(Skipped {
                            name: p.name.clone(),
                            reason: format!("{var} is not set"),
                        });
                        continue;
                    }
                },
                None => None,
            };
            let provider = Arc::new(ProviderConfig {
                name: p.name.clone(),
                base_url: p.base_url.clone(),
                api_key,
                headers: p
                    .headers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                stream_usage: p.stream_usage,
            });
            for m in &p.models {
                targets.push(Target {
                    provider: provider.clone(),
                    model: m.clone(),
                });
            }
        }
        (targets, skipped)
    }

    pub fn validate(&self) -> Result<()> {
        if self.agent.max_steps == 0 {
            bail!("agent.max_steps must be at least 1");
        }
        for p in &self.providers {
            if p.models.is_empty() {
                bail!("provider '{}' lists no models", p.name);
            }
            if !p.base_url.starts_with("http://") && !p.base_url.starts_with("https://") {
                bail!(
                    "provider '{}' base_url must start with http:// or https://",
                    p.name
                );
            }
        }
        Ok(())
    }
}

/// `~/.oblivion`, or `$OBLIVION_HOME` when set.
pub fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("OBLIVION_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oblivion")
}

pub fn default_config_path() -> PathBuf {
    home_dir().join("oblivion.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses_and_validates() {
        let c = Config::parse(DEFAULT_CONFIG).unwrap();
        c.validate().unwrap();
        assert_eq!(c.agent.approval, ApprovalMode::Ask);
        assert!(c.providers.len() >= 5);
    }

    #[test]
    fn targets_skip_missing_keys_and_disabled() {
        let c = Config::parse(DEFAULT_CONFIG).unwrap();
        let env = |k: &str| (k == "GROQ_API_KEY").then(|| "gsk_test".to_string());
        let (targets, skipped) = c.targets(&env);
        assert!(targets.iter().all(|t| t.provider.name == "groq"));
        assert_eq!(targets.len(), 2);
        assert!(skipped
            .iter()
            .any(|s| s.name == "openrouter" && s.reason.contains("OPENROUTER_API_KEY")));
        assert!(skipped
            .iter()
            .any(|s| s.name == "ollama" && s.reason.contains("disabled")));
    }
}
