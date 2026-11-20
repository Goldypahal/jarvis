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
#   checked = "2026-11-18"     date limits were last verified (YYYY-MM-DD)

[agent]
max_steps = 50          # tool-call rounds per request before stopping
approval = "ask"        # ask | auto-edit | yolo
context_tokens = 24000  # shrink the conversation when it grows past about this size
max_web_calls = 15      # web searches + fetches per request (sub-agents get 8 each)
small_model = false     # small-model mode: shorter prompt, fewer tools, strict repair
# shell = "powershell"  # powershell | cmd | bash | sh (default: powershell on Windows, bash elsewhere)

# Long-term memory across sessions.
# auto_extract: runs a cheap, budgeted model pass after sessions to propose memories.
# embedding_model: optional local embedding model through Ollama (e.g. "nomic-embed-text").
# When empty or unavailable, falls back to full-text search (FTS5).
[memory]
enabled = true
auto_extract = true
embedding_model = ""
ollama_endpoint = "http://localhost:11434"

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
checked = "2026-11-18"
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
checked = "2026-11-18"

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

# Which models do which jobs. Each value is a filter on "provider/model"; matching
# models are tried first, the rest stay as fallbacks. Use "ollama" to keep cheap work local.
# offline: when true, only local models (Ollama) are used; no data is sent to cloud.
[routing]
offline = false
summary = ""            # shrinking long conversations, e.g. "ollama"
subagent = ""           # sub-agents started by the `task` tool, e.g. "groq/openai/gpt-oss-20b"
subagent_max_steps = 25

[routing.rules]
private = ["ollama/*", "local/*"]
simple = ["ollama/*", "groq/llama-3.1-8b-instant", "openrouter/*"]
hard = ["groq/llama-3.3-70b-versatile", "openrouter/*", "ollama/*"]
vision = ["openrouter/*", "ollama/llava*", "ollama/*"]
long_context = ["openrouter/*", "groq/*", "ollama/*"]

# Voice input and speech-to-text
[voice]
enabled = true
stt_engine = "auto"         # "auto", "local-whisper", "groq", "openai", "web"
# whisper_path = ""         # path to whisper-cli / whisper.exe if not on PATH
hotkey = "Ctrl+Space"       # push-to-talk hotkey
language = "en"

# Skills: folders with SKILL.md from ~/.oblivion/skills, ~/.agents/skills, ~/.claude/skills,
# ~/.codex/skills and the project's .agents/skills (Claude and Codex skills work as-is).
[skills]
enabled = true
prompt_chars = 3200     # space for the skill list in every request (names + descriptions)

# Local models through Ollama (offline, private, no daily limits). Install from
# https://ollama.com, run `ollama pull qwen3.5:9b`, then set enabled = true.
# Ollama's default context is small: before starting it, set the environment variable
# OLLAMA_CONTEXT_LENGTH=16384 (or more), and keep max_input_tokens a bit below that.
# To use it for summaries and sub-agents, set summary = "ollama" / subagent = "ollama" above.
[[providers]]
name = "ollama"
base_url = "http://localhost:11434/v1"
models = ["qwen3.5:9b"]
max_input_tokens = 14000
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
    #[serde(default = "default_max_web_calls")]
    pub max_web_calls: usize,
    #[serde(default)]
    pub small_model: bool,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
            approval: ApprovalMode::Ask,
            shell: None,
            context_tokens: default_context_tokens(),
            max_web_calls: default_max_web_calls(),
            small_model: false,
        }
    }
}

fn default_max_steps() -> usize {
    50
}

fn default_max_web_calls() -> usize {
    15
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
pub struct RoutingSettings {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub subagent: String,
    #[serde(default = "default_subagent_steps")]
    pub subagent_max_steps: usize,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub rules: crate::routing::RoutingRules,
}

impl Default for RoutingSettings {
    fn default() -> Self {
        Self {
            summary: String::new(),
            subagent: String::new(),
            subagent_max_steps: default_subagent_steps(),
            offline: false,
            rules: crate::routing::RoutingRules::default(),
        }
    }
}

fn default_subagent_steps() -> usize {
    25
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
    3200
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
    #[serde(default)]
    pub checked: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub auto_extract: bool,
    #[serde(default = "default_extract_max_tokens")]
    pub max_tokens: usize,
    #[serde(default)]
    pub embedding_model: String,
    #[serde(default = "default_ollama_endpoint")]
    pub ollama_endpoint: String,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_extract: true,
            max_tokens: default_extract_max_tokens(),
            embedding_model: String::new(),
            ollama_endpoint: default_ollama_endpoint(),
        }
    }
}

fn default_extract_max_tokens() -> usize {
    2000
}

fn default_ollama_endpoint() -> String {
    "http://localhost:11434".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SetupSection {
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub setup: SetupSection,
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub memory: MemorySettings,
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub search: SearchSettings,
    #[serde(default)]
    pub skills: SkillSettings,
    #[serde(default)]
    pub routing: RoutingSettings,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerEntry>,
    #[serde(default)]
    pub browser: BrowserSection,
    #[serde(default)]
    pub voice: VoiceSettings,
}

/// `[voice]`: push-to-talk hotkey, local speech-to-text, and voice preferences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VoiceSettings {
    #[serde(default = "default_voice_enabled")]
    pub enabled: bool,
    #[serde(default = "default_voice_stt_engine")]
    pub stt_engine: String,
    #[serde(default)]
    pub whisper_path: Option<String>,
    #[serde(default = "default_voice_hotkey")]
    pub hotkey: String,
    #[serde(default = "default_voice_language")]
    pub language: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: default_voice_enabled(),
            stt_engine: default_voice_stt_engine(),
            whisper_path: None,
            hotkey: default_voice_hotkey(),
            language: default_voice_language(),
        }
    }
}

fn default_voice_enabled() -> bool {
    true
}

fn default_voice_stt_engine() -> String {
    "auto".to_string()
}

fn default_voice_hotkey() -> String {
    "Ctrl+Space".to_string()
}

fn default_voice_language() -> String {
    "en".to_string()
}

/// `[browser]`: a real web browser through Playwright MCP (needs Node.js).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSection {
    #[serde(default)]
    pub enabled: bool,
    /// "own": a separate browser window with its own saved logins.
    /// "extension": your Chrome/Edge tabs via the Playwright MCP Bridge extension.
    #[serde(default = "default_browser_mode")]
    pub mode: String,
    /// chrome (default), msedge, firefox, webkit
    #[serde(default)]
    pub browser: Option<String>,
    #[serde(default)]
    pub headless: bool,
    #[serde(default = "default_browser_command")]
    pub command: String,
    #[serde(default = "default_browser_package")]
    pub package: String,
    #[serde(default = "default_browser_timeout")]
    pub timeout_secs: u64,
}

impl Default for BrowserSection {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_browser_mode(),
            browser: None,
            headless: false,
            command: default_browser_command(),
            package: default_browser_package(),
            timeout_secs: default_browser_timeout(),
        }
    }
}

/// Turn the browser on in a config file's text: edit an existing `[browser]`
/// section in place (keeping comments), or append a new one.
pub fn enable_browser_in(text: &str, mode: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines.iter().position(|l| l.trim() == "[browser]") else {
        let mut out = text.trim_end().to_string();
        out.push('\n');
        out.push_str(&browser_config_block(mode));
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    let key_of = |l: &str| l.split('=').next().unwrap_or("").trim().to_string();
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let mut have_enabled = false;
    let mut have_mode = false;
    for line in out.iter_mut().take(end).skip(start + 1) {
        match key_of(line).as_str() {
            "enabled" => {
                *line = "enabled = true".into();
                have_enabled = true;
            }
            "mode" => {
                *line = format!("mode = \"{mode}\"");
                have_mode = true;
            }
            _ => {}
        }
    }
    let mut insert = Vec::new();
    if !have_enabled {
        insert.push("enabled = true".to_string());
    }
    if !have_mode {
        insert.push(format!("mode = \"{mode}\""));
    }
    for (i, l) in insert.into_iter().enumerate() {
        out.insert(start + 1 + i, l);
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Updates or inserts `[setup]` and `offline` settings in config text.
pub fn apply_setup_to_config_text(
    text: &str,
    completed: bool,
    workspace: Option<&str>,
    offline: Option<bool>,
) -> String {
    let mut out: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let setup_idx = out.iter().position(|l| l.trim() == "[setup]");

    if let Some(idx) = setup_idx {
        let mut i = idx + 1;
        let mut found_completed = false;
        let mut found_workspace = false;
        while i < out.len() && !out[i].trim().starts_with('[') {
            let trimmed = out[i].trim();
            if trimmed.starts_with("completed") {
                out[i] = format!("completed = {completed}");
                found_completed = true;
            } else if trimmed.starts_with("workspace") {
                if let Some(ws) = workspace {
                    out[i] = format!("workspace = \"{}\"", ws.replace('\\', "/"));
                    found_workspace = true;
                }
            }
            i += 1;
        }
        if !found_completed {
            out.insert(idx + 1, format!("completed = {completed}"));
        }
        if !found_workspace {
            if let Some(ws) = workspace {
                out.insert(
                    idx + 1,
                    format!("workspace = \"{}\"", ws.replace('\\', "/")),
                );
            }
        }
    } else {
        out.push(String::new());
        out.push("[setup]".to_string());
        out.push(format!("completed = {completed}"));
        if let Some(ws) = workspace {
            out.push(format!("workspace = \"{}\"", ws.replace('\\', "/")));
        }
    }

    if let Some(off) = offline {
        let routing_idx = out.iter().position(|l| l.trim() == "[routing]");
        if let Some(r_idx) = routing_idx {
            let mut i = r_idx + 1;
            let mut found_off = false;
            while i < out.len() && !out[i].trim().starts_with('[') {
                if out[i].trim().starts_with("offline") {
                    out[i] = format!("offline = {off}");
                    found_off = true;
                    break;
                }
                i += 1;
            }
            if !found_off {
                out.insert(r_idx + 1, format!("offline = {off}"));
            }
        } else {
            out.push(String::new());
            out.push("[routing]".to_string());
            out.push(format!("offline = {off}"));
        }
    }

    let mut res = out.join("\n");
    if !res.ends_with('\n') {
        res.push('\n');
    }
    res
}

fn default_browser_mode() -> String {
    "own".into()
}
fn default_browser_command() -> String {
    "npx".into()
}
fn default_browser_package() -> String {
    "@playwright/mcp@latest".into()
}
fn default_browser_timeout() -> u64 {
    90
}

/// The `[browser]` block `oblivion --setup-browser` adds to the config file.
pub fn browser_config_block(mode: &str) -> String {
    format!(
        r#"
# Web browser for the agent (Playwright MCP; needs Node.js). Set up with `oblivion --setup-browser`.
# mode "own": a separate browser window with its own saved logins (log in once, it remembers).
# mode "extension": your normal Chrome/Edge tabs; install the "Playwright MCP Bridge" extension.
# Clicking anything that submits, sends, pays or deletes always asks you first.
[browser]
enabled = true
mode = "{mode}"
# browser = "msedge"   # chrome (default) | msedge | firefox
# headless = false     # true = no visible window (own mode only)
"#
    )
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
                    checked: p.checked.clone(),
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
        if !matches!(self.browser.mode.as_str(), "own" | "extension") {
            bail!(
                "[browser] mode must be \"own\" or \"extension\", not \"{}\"",
                self.browser.mode
            );
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
            if let Some(date) = &p.checked {
                if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
                    bail!(
                        "provider '{}' has invalid checked date '{}'; expected YYYY-MM-DD",
                        p.name,
                        date
                    );
                }
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

    #[test]
    fn provider_checked_date_validation_and_propagation() {
        let toml = r#"
[[providers]]
name = "custom"
base_url = "https://custom.ai/api/v1"
models = ["m1"]
requests_per_day = 250
max_input_tokens = 8000
checked = "2026-11-18"
"#;
        let c = Config::parse(toml).unwrap();
        c.validate().unwrap();
        let (targets, _) = c.targets(&|_| Some("key".into()));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].requests_per_day, Some(250));
        assert_eq!(targets[0].max_input_tokens, Some(8000));
        assert_eq!(targets[0].checked.as_deref(), Some("2026-11-18"));

        let bad_toml = r#"
[[providers]]
name = "custom"
base_url = "https://custom.ai/api/v1"
models = ["m1"]
checked = "18-11-2026"
"#;
        let c_bad = Config::parse(bad_toml).unwrap();
        assert!(c_bad.validate().is_err());
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

    #[test]
    fn browser_section_is_added_or_edited_in_place() {
        let base = "[agent]\nmax_steps = 50\n";
        let added = enable_browser_in(base, "own");
        let cfg = Config::parse(&added).unwrap();
        assert!(cfg.browser.enabled);
        assert_eq!(cfg.browser.mode, "own");
        assert_eq!(cfg.agent.max_steps, 50);

        let existing = "[browser]\n# keep me\nenabled = false\nbrowser = \"msedge\"\n\n[skills]\nenabled = true\n";
        let edited = enable_browser_in(existing, "extension");
        assert!(edited.contains("# keep me"));
        let cfg = Config::parse(&edited).unwrap();
        assert!(cfg.browser.enabled);
        assert_eq!(cfg.browser.mode, "extension");
        assert_eq!(cfg.browser.browser.as_deref(), Some("msedge"));
        assert!(cfg.skills.enabled);
        assert!(!Config::parse("").unwrap().browser.enabled);
        assert!(Config::parse("[browser]\nmode = \"chrome\"\n")
            .unwrap()
            .validate()
            .is_err());
    }

    #[test]
    fn setup_section_apply_and_parse() {
        let base = "[agent]\nmax_steps = 50\n";
        let updated =
            apply_setup_to_config_text(base, true, Some("D:/Projects/Jarvis"), Some(true));
        let cfg = Config::parse(&updated).unwrap();
        assert!(cfg.setup.completed);
        assert_eq!(cfg.setup.workspace.as_deref(), Some("D:/Projects/Jarvis"));
        assert!(cfg.routing.offline);

        // Update existing setup section
        let reupdated =
            apply_setup_to_config_text(&updated, false, Some("C:/Workspace"), Some(false));
        let cfg2 = Config::parse(&reupdated).unwrap();
        assert!(!cfg2.setup.completed);
        assert_eq!(cfg2.setup.workspace.as_deref(), Some("C:/Workspace"));
        assert!(!cfg2.routing.offline);
    }
}
