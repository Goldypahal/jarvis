//! First-run onboarding wizard (Q5, Month 3).
//!
//! Provides guided configuration for initial workspace selection, model privacy choices
//! (local-only vs cloud API keys), and optional Windows autostart registration.

use anyhow::{Context, Result};
use oblivion_core::config::{apply_setup_to_config_text, Config, DEFAULT_CONFIG};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WizardStatus {
    pub completed: bool,
    pub workspace: String,
    pub offline: bool,
    pub autostart: bool,
    pub recommended_model: Option<String>,
    pub has_configured_keys: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct WizardCompleteRequest {
    pub workspace: Option<String>,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub api_keys: HashMap<String, String>,
}

pub fn get_wizard_status(config_path: &Path) -> WizardStatus {
    let current_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();

    let autostart = crate::autostart::get_status()
        .map(|s| s.enabled)
        .unwrap_or(false);

    let hardware = oblivion_core::hardware::detect_hardware();
    let rec = oblivion_core::hardware::recommend_model(&hardware);
    let sample_model = rec
        .recommended_models
        .first()
        .map(|s| s.as_str())
        .unwrap_or("qwen2.5-coder:7b");
    let recommended_model = Some(format!("{} (e.g. {})", rec.recommended_size, sample_model));

    if !config_path.exists() {
        return WizardStatus {
            completed: false,
            workspace: current_dir,
            offline: false,
            autostart,
            recommended_model,
            has_configured_keys: false,
        };
    }

    match Config::load(config_path) {
        Ok(cfg) => {
            let has_keys = cfg
                .providers
                .iter()
                .any(|p| crate::keys::for_provider(p).is_some());
            let ws = cfg
                .setup
                .workspace
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(current_dir);

            WizardStatus {
                completed: cfg.setup.completed,
                workspace: ws,
                offline: cfg.routing.offline,
                autostart,
                recommended_model,
                has_configured_keys: has_keys,
            }
        }
        Err(_) => WizardStatus {
            completed: false,
            workspace: current_dir,
            offline: false,
            autostart,
            recommended_model,
            has_configured_keys: false,
        },
    }
}

pub fn complete_wizard(config_path: &Path, req: &WizardCompleteRequest) -> Result<WizardStatus> {
    let ws = if let Some(ref w) = req.workspace {
        if !w.trim().is_empty() {
            let p = Path::new(w);
            if !p.exists() {
                std::fs::create_dir_all(p).with_context(|| {
                    format!("could not create workspace folder {}", p.display())
                })?;
            }
            w.clone()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .to_string_lossy()
                .to_string()
        }
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    };

    // Save provided API keys
    for (provider, key) in &req.api_keys {
        if !key.trim().is_empty() {
            let _ = crate::keys::save(provider, key);
        }
    }

    // Set autostart if requested
    let _ = crate::autostart::set_enabled(req.autostart);

    // Update config file
    let base_text = if config_path.exists() {
        std::fs::read_to_string(config_path).unwrap_or_else(|_| DEFAULT_CONFIG.to_string())
    } else {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        DEFAULT_CONFIG.to_string()
    };

    let updated = apply_setup_to_config_text(&base_text, true, Some(&ws), Some(req.offline));
    std::fs::write(config_path, updated)
        .with_context(|| format!("could not write config to {}", config_path.display()))?;

    Ok(get_wizard_status(config_path))
}

pub fn run_cli_wizard(config_path: &Path) -> Result<()> {
    println!("=== Jarvis & Oblivion Setup Wizard ===\n");
    println!("Welcome! Let's get your assistant ready in under a minute.\n");

    let current = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .to_string();

    println!("Step 1: Workspace Folder");
    println!("Where should Jarvis work on files and projects? [Default: {current}]");
    let mut ws_input = String::new();
    let _ = std::io::stdin().read_line(&mut ws_input);
    let ws = if ws_input.trim().is_empty() {
        current
    } else {
        ws_input.trim().to_string()
    };

    println!("\nStep 2: Privacy & Intelligence Mode");
    println!("1) Local Models Only (100% offline via Ollama, zero data leaves PC)");
    println!("2) Cloud / Free-tier APIs (Groq, OpenRouter, Gemini, etc.)");
    println!("3) Hybrid (Local for private tasks, Cloud for hard tasks) [Default]");
    let mut mode_input = String::new();
    let _ = std::io::stdin().read_line(&mut mode_input);
    let offline = mode_input.trim() == "1";

    println!("\nStep 3: Windows Startup");
    println!("Launch Jarvis web background service when Windows starts? (y/N): ");
    let mut auto_input = String::new();
    let _ = std::io::stdin().read_line(&mut auto_input);
    let autostart = auto_input.trim().eq_ignore_ascii_case("y");

    let req = WizardCompleteRequest {
        workspace: Some(ws),
        offline,
        autostart,
        api_keys: HashMap::new(),
    };

    let status = complete_wizard(config_path, &req)?;
    println!("\nSetup complete!");
    println!("  - Workspace: {}", status.workspace);
    println!("  - Offline Mode: {}", status.offline);
    println!("  - Autostart: {}", status.autostart);
    if let Some(ref m) = status.recommended_model {
        println!("  - Recommended Model: {m}");
    }
    println!("\nYou are ready! Run `oblivion --web` to launch the Executive Desktop.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_status_and_completion_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("oblivion.toml");

        // 1. Initial state without config
        let s0 = get_wizard_status(&cfg_path);
        assert!(!s0.completed);
        assert!(!s0.offline);

        // 2. Complete wizard with custom workspace and offline mode
        let test_ws = dir.path().join("my_workspace");
        let req = WizardCompleteRequest {
            workspace: Some(test_ws.to_string_lossy().to_string()),
            offline: true,
            autostart: false,
            api_keys: HashMap::new(),
        };

        let s1 = complete_wizard(&cfg_path, &req).unwrap();
        assert!(s1.completed);
        assert!(s1.offline);
        assert_eq!(Path::new(&s1.workspace), test_ws.as_path());
        assert!(test_ws.exists());

        // 3. Re-read status from disk to ensure persistence
        let s2 = get_wizard_status(&cfg_path);
        assert!(s2.completed);
        assert!(s2.offline);
        assert_eq!(Path::new(&s2.workspace), test_ws.as_path());
    }
}
