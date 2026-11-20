//! Windows startup and autostart management (SRS FR-104).
//!
//! Manages registration in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! so that Jarvis / Oblivion can optionally start in background web mode on user login.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub command: Option<String>,
    pub value_name: String,
}

pub struct AutostartManager {
    value_name: String,
}

impl Default for AutostartManager {
    fn default() -> Self {
        Self::new("Jarvis")
    }
}

impl AutostartManager {
    pub fn new(value_name: impl Into<String>) -> Self {
        Self {
            value_name: value_name.into(),
        }
    }

    #[cfg(windows)]
    pub fn status(&self) -> Result<AutostartStatus> {
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                &self.value_name,
            ])
            .output()
            .context("failed to query registry for autostart status")?;

        if !output.status.success() {
            return Ok(AutostartStatus {
                enabled: false,
                command: None,
                value_name: self.value_name.clone(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(&self.value_name) {
                if let Some(pos) = trimmed.find("REG_SZ") {
                    let cmd = trimmed[pos + 6..].trim().to_string();
                    return Ok(AutostartStatus {
                        enabled: true,
                        command: Some(cmd),
                        value_name: self.value_name.clone(),
                    });
                }
            }
        }

        Ok(AutostartStatus {
            enabled: false,
            command: None,
            value_name: self.value_name.clone(),
        })
    }

    #[cfg(not(windows))]
    pub fn status(&self) -> Result<AutostartStatus> {
        Ok(AutostartStatus {
            enabled: false,
            command: None,
            value_name: self.value_name.clone(),
        })
    }

    #[cfg(windows)]
    pub fn set_enabled(
        &self,
        enable: bool,
        custom_command: Option<&str>,
    ) -> Result<AutostartStatus> {
        if enable {
            let cmd = if let Some(c) = custom_command {
                c.to_string()
            } else {
                let exe = std::env::current_exe()
                    .context("could not determine current executable path")?;
                format!("\"{}\" --web --no-open", exe.display())
            };

            let status = std::process::Command::new("reg")
                .args([
                    "add",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    &self.value_name,
                    "/t",
                    "REG_SZ",
                    "/d",
                    &cmd,
                    "/f",
                ])
                .status()
                .context("failed to add autostart registry entry")?;

            if !status.success() {
                bail!("reg add exited with status: {status}");
            }

            Ok(AutostartStatus {
                enabled: true,
                command: Some(cmd),
                value_name: self.value_name.clone(),
            })
        } else {
            let _ = std::process::Command::new("reg")
                .args([
                    "delete",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                    "/v",
                    &self.value_name,
                    "/f",
                ])
                .status();

            Ok(AutostartStatus {
                enabled: false,
                command: None,
                value_name: self.value_name.clone(),
            })
        }
    }

    #[cfg(not(windows))]
    pub fn set_enabled(
        &self,
        _enable: bool,
        _custom_command: Option<&str>,
    ) -> Result<AutostartStatus> {
        bail!("Autostart is currently only supported on Windows")
    }
}

pub fn get_status() -> Result<AutostartStatus> {
    AutostartManager::default().status()
}

pub fn set_enabled(enable: bool) -> Result<AutostartStatus> {
    AutostartManager::default().set_enabled(enable, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn test_autostart_lifecycle() {
        let test_name = "JarvisAutostartTestKey_Ephemeral";
        let mgr = AutostartManager::new(test_name);

        // Ensure clean initial state
        let _ = mgr.set_enabled(false, None);
        let s0 = mgr.status().unwrap();
        assert!(!s0.enabled);
        assert_eq!(s0.command, None);

        // Enable with a test command
        let s1 = mgr
            .set_enabled(true, Some("\"C:\\Test\\jarvis.exe\" --web --no-open"))
            .unwrap();
        assert!(s1.enabled);
        assert_eq!(
            s1.command.as_deref(),
            Some("\"C:\\Test\\jarvis.exe\" --web --no-open")
        );

        // Verify status reflects enabled
        let s2 = mgr.status().unwrap();
        assert!(s2.enabled);
        assert_eq!(
            s2.command.as_deref(),
            Some("\"C:\\Test\\jarvis.exe\" --web --no-open")
        );

        // Disable and verify clean
        let s3 = mgr.set_enabled(false, None).unwrap();
        assert!(!s3.enabled);
        assert_eq!(s3.command, None);

        let s4 = mgr.status().unwrap();
        assert!(!s4.enabled);
    }
}
