//! Connector management for MCP servers (Month 6).
//!
//! Provides configuration, preset templates (Gmail, Calendar, GitHub, Notion, Filesystem),
//! and Windows Credential Manager integration for secure API keys and tokens.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use oblivion_core::config::{Config, McpServerEntry};
use serde::{Deserialize, Serialize};

use crate::keys;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectorInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub kind: String,
    pub transport_type: String, // "stdio" | "http"
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub token_env: Option<String>,
    pub enabled: bool,
    pub has_key: bool,
    pub key_source: Option<String>, // "credential_manager" | "env" | None
    pub needs_key: bool,
    pub is_preset: bool,
}

pub struct PresetConnector {
    pub name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub kind: &'static str,
    pub transport_type: &'static str,
    pub default_command: Option<&'static str>,
    pub default_args: &'static [&'static str],
    pub default_url: Option<&'static str>,
    pub token_env: Option<&'static str>,
    pub needs_key: bool,
}

pub const PRESETS: &[PresetConnector] = &[
    PresetConnector {
        name: "github",
        display_name: "GitHub",
        description: "Search code, inspect repositories, manage pull requests and issues",
        kind: "github",
        transport_type: "http",
        default_command: None,
        default_args: &[],
        default_url: Some("https://api.githubcopilot.com/mcp/"),
        token_env: Some("GITHUB_TOKEN"),
        needs_key: true,
    },
    PresetConnector {
        name: "notion",
        display_name: "Notion",
        description: "Read and search workspace pages, databases, and project notes",
        kind: "notion",
        transport_type: "http",
        default_command: None,
        default_args: &[],
        default_url: Some("https://mcp.notion.com/v1"),
        token_env: Some("NOTION_API_KEY"),
        needs_key: true,
    },
    PresetConnector {
        name: "gmail",
        display_name: "Gmail",
        description: "Search, summarize, and draft emails from your Google Workspace account",
        kind: "gmail",
        transport_type: "http",
        default_command: None,
        default_args: &[],
        default_url: Some("https://mcp.google.com/gmail"),
        token_env: Some("GMAIL_API_KEY"),
        needs_key: true,
    },
    PresetConnector {
        name: "calendar",
        display_name: "Google Calendar",
        description: "Query upcoming meetings, manage agenda, and check schedule availability",
        kind: "calendar",
        transport_type: "http",
        default_command: None,
        default_args: &[],
        default_url: Some("https://mcp.google.com/calendar"),
        token_env: Some("GOOGLE_CALENDAR_KEY"),
        needs_key: true,
    },
    PresetConnector {
        name: "files",
        display_name: "Filesystem",
        description: "Read, search, and navigate local directories and documents securely",
        kind: "files",
        transport_type: "stdio",
        default_command: Some("npx"),
        default_args: &["-y", "@modelcontextprotocol/server-filesystem", "."],
        default_url: None,
        token_env: None,
        needs_key: false,
    },
];

/// Lists all connectors (combining configured MCP servers from `oblivion.toml` with starter presets).
pub fn list_connectors(config: &Config) -> Vec<ConnectorInfo> {
    let mut result = Vec::new();

    // 1. Process configured servers from oblivion.toml
    let mut configured_names = std::collections::HashSet::new();
    for s in &config.mcp_servers {
        configured_names.insert(s.name.clone());

        // Check if matches a preset
        let preset = PRESETS.iter().find(|p| p.name == s.name);
        let display_name = preset
            .map(|p| p.display_name.to_string())
            .unwrap_or_else(|| s.name.clone());
        let kind = preset
            .map(|p| p.kind.to_string())
            .unwrap_or_else(|| "custom".to_string());
        let needs_key = preset.map(|p| p.needs_key).unwrap_or(s.url.is_some());

        let (has_key, key_source) = match keys::get_connector_key(&s.name, s.token_env.as_deref()) {
            Some((_, keys::KeySource::Env)) => (true, Some("env".to_string())),
            Some((_, keys::KeySource::Store)) => (true, Some("credential_manager".to_string())),
            None => (false, None),
        };

        let transport_type = if s.url.is_some() {
            "http".to_string()
        } else {
            "stdio".to_string()
        };

        result.push(ConnectorInfo {
            name: s.name.clone(),
            display_name,
            description: s.description.clone().unwrap_or_else(|| {
                preset
                    .map(|p| p.description.to_string())
                    .unwrap_or_default()
            }),
            kind,
            transport_type,
            command: s.command.clone(),
            args: s.args.clone(),
            url: s.url.clone(),
            token_env: s.token_env.clone(),
            enabled: s.enabled,
            has_key,
            key_source,
            needs_key,
            is_preset: preset.is_some(),
        });
    }

    // 2. Add unconfigured presets so users can easily enable and configure them
    for p in PRESETS {
        if !configured_names.contains(p.name) {
            let (has_key, key_source) = match keys::get_connector_key(p.name, p.token_env) {
                Some((_, keys::KeySource::Env)) => (true, Some("env".to_string())),
                Some((_, keys::KeySource::Store)) => (true, Some("credential_manager".to_string())),
                None => (false, None),
            };

            result.push(ConnectorInfo {
                name: p.name.to_string(),
                display_name: p.display_name.to_string(),
                description: p.description.to_string(),
                kind: p.kind.to_string(),
                transport_type: p.transport_type.to_string(),
                command: p.default_command.map(String::from),
                args: p.default_args.iter().map(|&s| s.to_string()).collect(),
                url: p.default_url.map(String::from),
                token_env: p.token_env.map(String::from),
                enabled: false,
                has_key,
                key_source,
                needs_key: p.needs_key,
                is_preset: true,
            });
        }
    }

    result
}

/// Toggles or adds a connector in `oblivion.toml`.
pub fn toggle_connector(config_path: &Path, name: &str, enabled: bool) -> Result<ConnectorInfo> {
    let mut config = if config_path.exists() {
        Config::load(config_path).context("loading config file")?
    } else {
        Config::default()
    };

    if let Some(entry) = config.mcp_servers.iter_mut().find(|m| m.name == name) {
        entry.enabled = enabled;
    } else if let Some(p) = PRESETS.iter().find(|p| p.name == name) {
        // Add preset entry
        let new_entry = McpServerEntry {
            name: p.name.to_string(),
            description: Some(p.description.to_string()),
            command: p.default_command.map(String::from),
            args: p.default_args.iter().map(|&s| s.to_string()).collect(),
            env: BTreeMap::new(),
            url: p.default_url.map(String::from),
            token_env: p.token_env.map(String::from),
            enabled,
            timeout_secs: 30,
        };
        config.mcp_servers.push(new_entry);
    } else {
        bail!("Unknown connector: {name}");
    }

    // Save updated config
    let toml_str = toml::to_string_pretty(&config).context("serializing updated config to TOML")?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(config_path, toml_str).context("saving updated config to disk")?;

    let list = list_connectors(&config);
    list.into_iter()
        .find(|c| c.name == name)
        .context("retrieving updated connector")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_connectors_includes_all_presets() {
        let config = Config::default();
        let connectors = list_connectors(&config);

        assert!(connectors.len() >= 5);
        let names: Vec<String> = connectors.iter().map(|c| c.name.clone()).collect();
        assert!(names.contains(&"github".to_string()));
        assert!(names.contains(&"notion".to_string()));
        assert!(names.contains(&"gmail".to_string()));
        assert!(names.contains(&"calendar".to_string()));
        assert!(names.contains(&"files".to_string()));
    }

    #[test]
    fn test_toggle_connector_persists_to_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("oblivion.toml");

        // Toggle github enabled
        let c1 = toggle_connector(&cfg_path, "github", true).unwrap();
        assert!(c1.enabled);
        assert_eq!(c1.name, "github");

        // Verify loaded config reflects enabled
        let loaded = Config::load(&cfg_path).unwrap();
        let server = loaded
            .mcp_servers
            .iter()
            .find(|s| s.name == "github")
            .unwrap();
        assert!(server.enabled);

        // Toggle github disabled
        let c2 = toggle_connector(&cfg_path, "github", false).unwrap();
        assert!(!c2.enabled);
    }
}
