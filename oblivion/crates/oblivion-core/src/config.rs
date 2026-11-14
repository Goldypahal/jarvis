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
# API keys: run `oblivion --set-key <provider>` (stored in Windows Credential Manager),
# or set the environment variable named in api_key_env. Never put keys in this file.
#
# Free-tier limits (optional per provider; built-in defaults exist for openrouter and groq):
#   requests_per_day = 1000    skip this provider once today's allowance is used
#   max_input_tokens = 7000    send bigger requests to another provider instead

[agent]
max_steps = 50          # tool-call rounds per request before stopping
approval = "ask"        # ask | auto-edit | yolo
context_tokens = 24000  # shrink the conversation when it grows past about this size
# shell = "powershell"  # powershell | cmd | bash | sh (default: powershell on Windows, bash elsewhere)

# Rules are "tool(pattern)"; * matches anything. Deny wins over allow.
# Allowed calls run without asking; denied calls are refused.
[permissions]
allow = ["shell(git status*)", "shell(git diff*)", "shell(git log*)", "shell(cargo test*)", "shell(cargo build*)", "shell(pytest*)", "shell(python -m pytest*)"]
deny = ["shell(*rm -rf /*)", "shell(*rm -rf ~*)", "shell(*git push --force*)", "shell(*git push -f *)", "shell(format *)", "shell(*Format-Volume*)"]

# web_search backend: "duckduckgo" (no key), "tavily" (free monthly credits; key via
# `oblivion --set-key tavily` or TAVILY_API_KEY) or "searxng" (your own server).
[search]
backend = "duckduckgo"
# searxng_url = "http://localhost:8888"

# OpenRouter: 20 req/min, 50 req/day on ":free" models (1,000/day after a one-time $10 credit).
# Keys: https://openrouter.ai/keys
[[providers]]
name = "openrouter"
base_url = "https://openrouter.ai/api/v1"
api_key_env = "OPENROUTER_API_KEY"
models = ["qwen/qwen3-coder:free", "openai/gpt-oss-120b:free"]
requests_per_day = 50
headers = { "HTTP-Referer" = "https://github.com/Goldypahal/jarvis", "X-Title" = "Oblivion" }

# Groq: fast; per model about 30 req/min, 1,000 req/day and only ~8,000 tokens/min on
# the free plan, so big requests go elsewhere. Keys: https://console.groq.com/keys
[[providers]]
name = "groq"
base_url = "https://api.groq.com/openai/v1"
api_key_env = "GROQ_API_KEY"
models = ["openai/gpt-oss-120b", "openai/gpt-oss-20b"]
requests_per_day = 1000
max_input_tokens = 7000

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

# MCP connectors. Each server is a local program (command + args) or a URL.
# The model sees one compact `mcp` tool and asks for a server's tools only when needed.
# Tokens for URL servers: `oblivion --set-key mcp-<name>` or the env var in token_env.
#
# [[mcp_servers]]
# name = "filesystem"
# description = "Read and write files in my Documents folder"
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me\\Documents"]
#
# [[mcp_servers]]
# name = "github"
# description = "GitHub issues, pull requests and repositories"
# url = "https://api.githubcopilot.com/mcp/"
# token_env = "GITHUB_TOKEN"

# Skills: folders with SKILL.md from ~/.oblivion/skills, ~/.agents/skills, ~/.claude/skills,
# ~/.codex/skills and the project's .agents/skills (Claude and Codex skills work as-is).
[skills]
enabled = true
prompt_chars = 2500     # space for the skill list in every request (names + descriptions)

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
    #[serde(default = "default_context_tokens")]
    pub context_tokens: usize,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
            approval: ApprovalMode::Ask,
            shell: None,
            context_tokens: default_context_tokens(),
        }
    }
}

fn default_max_steps() -> usize {
    50
}

fn default_context_tokens() -> usize {
    24_000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_skill_chars")]
    pub prompt_chars: usize,
}

impl Default for SkillSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            prompt_chars: default_skill_chars(),
        }
    }
}

fn default_skill_chars() -> usize {
    2500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Local server: program to run (e.g. "npx", "uvx", "python").
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Remote server: streamable-HTTP URL.
    #[serde(default)]
    pub url: Option<String>,
    /// Environment variable with a bearer token for `url` servers.
    #[serde(default)]
    pub token_env: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_mcp_timeout")]
    pub timeout_secs: u64,
}

fn default_mcp_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSettings {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default)]
    pub searxng_url: Option<String>,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            backend: default_backend(),
            searxng_url: None,
        }
    }
}

fn default_backend() -> String {
    "duckduckgo".into()
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
    #[serde(default)]
    pub requests_per_day: Option<u32>,
    #[serde(default)]
    pub max_input_tokens: Option<u32>,
}

/// Free-tier limits used when the config does not set them (Nov 2026 figures).
fn known_limits(provider: &str) -> (Option<u32>, Option<u32>) {
    match provider {
        "openrouter" => (Some(50), None),
        "groq" => (Some(1000), Some(7000)),
        _ => (None, None),
    }
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
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub search: SearchSettings,
    #[serde(default)]
    pub skills: SkillSettings,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerEntry>,
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
    /// `key_for` looks up a provider's key (environment variable, then key store).
    pub fn targets(
        &self,
        key_for: &dyn Fn(&ProviderEntry) -> Option<String>,
    ) -> (Vec<Target>, Vec<Skipped>) {
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
                Some(var) => match key_for(p).filter(|v| !v.trim().is_empty()) {
                    Some(k) => Some(k.trim().to_string()),
                    None => {
                        skipped.push(Skipped {
                            name: p.name.clone(),
                            reason: format!(
                                "no API key (run `oblivion --set-key {}` or set {var})",
                                p.name
                            ),
                        });
                        continue;
                    }
                },
                None => None,
            };
            let (known_rpd, known_max) = known_limits(&p.name);
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
                    requests_per_day: p.requests_per_day.or(known_rpd),
                    max_input_tokens: p.max_input_tokens.or(known_max),
                    ..Target::new(provider.clone(), m.clone())
                });
            }
        }
        (targets, skipped)
    }

    pub fn validate(&self) -> Result<()> {
        if self.agent.max_steps == 0 {
            bail!("agent.max_steps must be at least 1");
        }
        for m in &self.mcp_servers {
            match (&m.command, &m.url) {
                (Some(_), None) | (None, Some(_)) => {}
                _ => bail!(
                    "mcp server '{}' needs exactly one of `command` or `url`",
                    m.name
                ),
            }
            if m.name.is_empty() || m.name.contains(char::is_whitespace) {
                bail!("mcp server names must be one word, got '{}'", m.name);
            }
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
        let keys = |p: &ProviderEntry| (p.name == "groq").then(|| "gsk_test".to_string());
        let (targets, skipped) = c.targets(&keys);
        assert!(targets.iter().all(|t| t.provider.name == "groq"));
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].max_input_tokens, Some(7000));
        assert_eq!(targets[0].requests_per_day, Some(1000));
        assert_eq!(c.permissions.deny.len(), 6);
        assert_eq!(c.search.backend, "duckduckgo");
        assert!(skipped
            .iter()
            .any(|s| s.name == "openrouter" && s.reason.contains("--set-key openrouter")));
        assert!(skipped
            .iter()
            .any(|s| s.name == "ollama" && s.reason.contains("disabled")));
    }
}

#[cfg(test)]
mod compat_tests {
    use super::*;

    #[test]
    fn old_phase0_config_still_loads_with_known_limits() {
        let old = r#"
[agent]
max_steps = 50
approval = "ask"
[[providers]]
name = "groq"
base_url = "https://api.groq.com/openai/v1"
api_key_env = "GROQ_API_KEY"
models = ["openai/gpt-oss-120b"]
"#;
        let c = Config::parse(old).unwrap();
        assert_eq!(c.agent.context_tokens, 24_000);
        assert!(c.permissions.allow.is_empty());
        let (t, _) = c.targets(&|_| Some("k".into()));
        assert_eq!(t[0].max_input_tokens, Some(7000));
    }
}
