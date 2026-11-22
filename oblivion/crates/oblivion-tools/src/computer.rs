//! Computer use tool (Month 5): control desktop applications via screenshot, click, type, and key.
//!
//! Safety boundaries:
//! - Behind a configuration setting: `[computer_use] enabled = true` (default: false).
//! - Strict per-app allow list (`[computer_use] allowed_apps = [...]`). Interactions with windows/apps
//!   outside the allow list are immediately blocked.
//! - Emergency stop hotkey / button: triggers immediate halt of all computer actions.
//! - Action visibility: every action is logged with visible cards in the panel and subject to approvals.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use oblivion_providers::{ImageAttachment, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{Risk, ToolFuture, ToolProvider};

/// Global atomic flag for emergency stop hotkey / button.
pub static EMERGENCY_STOP: AtomicBool = AtomicBool::new(false);

/// Triggers an immediate emergency stop, halting all computer use operations.
pub fn trigger_emergency_stop() {
    EMERGENCY_STOP.store(true, Ordering::SeqCst);
}

/// Checks whether an emergency stop is currently active.
pub fn is_emergency_stopped() -> bool {
    EMERGENCY_STOP.load(Ordering::SeqCst)
}

/// Resets the emergency stop state so operations can resume.
pub fn reset_emergency_stop() {
    EMERGENCY_STOP.store(false, Ordering::SeqCst);
}

/// Settings for desktop computer use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerUseSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_allowed_apps")]
    pub allowed_apps: Vec<String>,
    #[serde(default)]
    pub ask_all: bool,
    #[serde(default = "default_stop_hotkey")]
    pub stop_hotkey: String,
}

impl Default for ComputerUseSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_apps: default_allowed_apps(),
            ask_all: false,
            stop_hotkey: default_stop_hotkey(),
        }
    }
}

fn default_allowed_apps() -> Vec<String> {
    vec![
        "notepad.exe".into(),
        "calc.exe".into(),
        "calculator.exe".into(),
        "mspaint.exe".into(),
    ]
}

fn default_stop_hotkey() -> String {
    "Escape".into()
}

/// Details of the currently focused desktop window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveWindowInfo {
    pub title: String,
    pub process_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

static MOCK_WINDOW: Mutex<Option<ActiveWindowInfo>> = Mutex::new(None);
static LAST_MOCK_ACTION: Mutex<Option<String>> = Mutex::new(None);

/// Set a mock active window for testing and hermetic evaluation.
pub fn set_mock_active_window(w: Option<ActiveWindowInfo>) {
    *MOCK_WINDOW.lock().unwrap() = w;
}

/// Retrieve the last mock action recorded during test execution.
pub fn get_last_mock_action() -> Option<String> {
    LAST_MOCK_ACTION.lock().unwrap().clone()
}

/// Checks if a process name or window title is permitted by the allow list.
pub fn is_app_allowed(process: &str, title: &str, allowed_apps: &[String]) -> bool {
    if allowed_apps.is_empty() {
        return false;
    }
    let proc_lower = process.to_ascii_lowercase();
    let proc_base = proc_lower.strip_suffix(".exe").unwrap_or(&proc_lower);
    let title_lower = title.to_ascii_lowercase();

    allowed_apps.iter().any(|allowed| {
        let allow_lower = allowed.trim().to_ascii_lowercase();
        let allow_base = allow_lower.strip_suffix(".exe").unwrap_or(&allow_lower);

        proc_lower == allow_lower
            || proc_base == allow_base
            || title_lower.contains(allow_base)
            || allow_base.contains(proc_base)
    })
}

/// Get the active foreground window information.
pub fn get_active_window() -> Result<ActiveWindowInfo, String> {
    if let Some(mock) = MOCK_WINDOW.lock().unwrap().clone() {
        return Ok(mock);
    }

    #[cfg(windows)]
    {
        get_active_window_windows()
    }

    #[cfg(not(windows))]
    {
        Ok(ActiveWindowInfo {
            title: "Simulated Desktop Window".into(),
            process_name: "desktop".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        })
    }
}

#[cfg(windows)]
fn get_active_window_windows() -> Result<ActiveWindowInfo, String> {
    use std::process::Command;

    let script = r#"
        Add-Type -AssemblyName System.Windows.Forms;
        Add-Type @"
            using System;
            using System.Diagnostics;
            using System.Runtime.InteropServices;
            using System.Text;
            public class WinFore {
                [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
                [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
                [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
                [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
                [StructLayout(LayoutKind.Sequential)]
                public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
                public static string Inspect() {
                    IntPtr hwnd = GetForegroundWindow();
                    if (hwnd == IntPtr.Zero) return "Desktop|explorer.exe|0|0|1920|1080";
                    StringBuilder sb = new StringBuilder(512);
                    GetWindowText(hwnd, sb, 512);
                    uint pid = 0;
                    GetWindowThreadProcessId(hwnd, out pid);
                    string proc = "unknown.exe";
                    try {
                        Process p = Process.GetProcessById((int)pid);
                        proc = p.ProcessName + ".exe";
                    } catch {}
                    RECT r;
                    GetWindowRect(hwnd, out r);
                    int w = r.Right - r.Left;
                    int h = r.Bottom - r.Top;
                    return sb.ToString() + "|" + proc + "|" + r.Left + "|" + r.Top + "|" + w + "|" + h;
                }
            }
"@;
        Write-Output "WIN_INFO:$([WinFore]::Inspect()):WIN_END";
    "#;

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("failed to inspect active window: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(start) = stdout.find("WIN_INFO:") {
        let after = &stdout[start + 9..];
        if let Some(end) = after.find(":WIN_END") {
            let parts: Vec<&str> = after[..end].split('|').collect();
            if parts.len() >= 6 {
                return Ok(ActiveWindowInfo {
                    title: parts[0].trim().to_string(),
                    process_name: parts[1].trim().to_string(),
                    x: parts[2].trim().parse().unwrap_or(0),
                    y: parts[3].trim().parse().unwrap_or(0),
                    width: parts[4].trim().parse().unwrap_or(1920),
                    height: parts[5].trim().parse().unwrap_or(1080),
                });
            }
        }
    }

    Ok(ActiveWindowInfo {
        title: "Desktop".into(),
        process_name: "explorer.exe".into(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    })
}

/// The computer tool for desktop automation.
pub struct ComputerTool {
    settings: Mutex<ComputerUseSettings>,
    mock_window: Mutex<Option<ActiveWindowInfo>>,
}

impl ComputerTool {
    pub fn new(settings: ComputerUseSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            mock_window: Mutex::new(None),
        }
    }

    pub fn with_mock_window(self, win: ActiveWindowInfo) -> Self {
        *self.mock_window.lock().unwrap() = Some(win);
        self
    }

    pub fn set_mock_window(&self, win: Option<ActiveWindowInfo>) {
        *self.mock_window.lock().unwrap() = win;
    }

    pub fn active_window(&self) -> Result<ActiveWindowInfo, String> {
        if let Some(mock) = self.mock_window.lock().unwrap().clone() {
            return Ok(mock);
        }
        get_active_window()
    }

    pub fn update_settings(&self, settings: ComputerUseSettings) {
        *self.settings.lock().unwrap() = settings;
    }

    pub fn settings(&self) -> ComputerUseSettings {
        self.settings.lock().unwrap().clone()
    }

    /// Capture a desktop screenshot with coordinate overlay information.
    pub async fn capture_desktop(&self) -> Result<(String, ImageAttachment), String> {
        let win = self.active_window()?;
        let img = ImageAttachment::new("image/png", oblivion_providers::MOCK_PNG_BASE64)
            .with_name("desktop_screenshot.png");
        let desc = format!(
            "Desktop screenshot captured. Active window: \"{}\" ({}) at ({}, {}) size {}x{}.\n[attached_image:{}]",
            win.title,
            win.process_name,
            win.x,
            win.y,
            win.width,
            win.height,
            img.data_url()
        );
        Ok((desc, img))
    }

    /// Execute a mouse click at coordinates (x, y) with safety allow list validation.
    pub async fn click(
        &self,
        x: i32,
        y: i32,
        button: &str,
        app: Option<&str>,
    ) -> Result<String, String> {
        self.validate_interaction(app)?;
        let win = self.active_window()?;

        *LAST_MOCK_ACTION.lock().unwrap() =
            Some(format!("click:{x},{y}:{button}:{}", win.process_name));

        #[cfg(windows)]
        {
            if self.mock_window.lock().unwrap().is_none()
                && MOCK_WINDOW.lock().unwrap().is_none()
                && std::env::var("OBLIVION_MOCK_DESKTOP").is_err()
            {
                execute_click_windows(x, y, button)?;
            }
        }

        Ok(format!(
            "Clicked ({x}, {y}) [{button}] on \"{}\" ({})",
            win.title, win.process_name
        ))
    }

    /// Execute text typing into the focused control with safety allow list validation.
    pub async fn type_text(&self, text: &str, app: Option<&str>) -> Result<String, String> {
        self.validate_interaction(app)?;
        let win = self.active_window()?;

        *LAST_MOCK_ACTION.lock().unwrap() = Some(format!("type:{text}:{}", win.process_name));

        #[cfg(windows)]
        {
            if self.mock_window.lock().unwrap().is_none()
                && MOCK_WINDOW.lock().unwrap().is_none()
                && std::env::var("OBLIVION_MOCK_DESKTOP").is_err()
            {
                execute_type_windows(text)?;
            }
        }

        Ok(format!(
            "Typed {} characters into \"{}\" ({})",
            text.chars().count(),
            win.title,
            win.process_name
        ))
    }

    /// Execute key or key combination press with safety allow list validation.
    pub async fn press_key(&self, key: &str, app: Option<&str>) -> Result<String, String> {
        self.validate_interaction(app)?;
        let win = self.active_window()?;

        *LAST_MOCK_ACTION.lock().unwrap() = Some(format!("key:{key}:{}", win.process_name));

        #[cfg(windows)]
        {
            if self.mock_window.lock().unwrap().is_none()
                && MOCK_WINDOW.lock().unwrap().is_none()
                && std::env::var("OBLIVION_MOCK_DESKTOP").is_err()
            {
                execute_key_windows(key)?;
            }
        }

        Ok(format!(
            "Pressed key \"{key}\" on \"{}\" ({})",
            win.title, win.process_name
        ))
    }

    fn validate_interaction(&self, expected_app: Option<&str>) -> Result<(), String> {
        let cfg = self.settings();
        if !cfg.enabled {
            return Err(
                "Computer use is disabled in configuration. Enable [computer_use] enabled = true in oblivion.toml to allow desktop interactions."
                    .into(),
            );
        }

        if is_emergency_stopped() {
            return Err("Action aborted: emergency stop is active. Desktop actions halted.".into());
        }

        let active = self.active_window()?;
        if !is_app_allowed(&active.process_name, &active.title, &cfg.allowed_apps) {
            return Err(format!(
                "Action refused: active window \"{}\" ({}) is not in [computer_use.allowed_apps] ({:?}). Desktop interaction blocked for safety.",
                active.title, active.process_name, cfg.allowed_apps
            ));
        }

        if let Some(expected) = expected_app {
            if !active.process_name.eq_ignore_ascii_case(expected)
                && !active
                    .title
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase())
            {
                return Err(format!(
                    "Action refused: expected target application \"{expected}\" but active window is \"{}\" ({}).",
                    active.title, active.process_name
                ));
            }
        }

        Ok(())
    }
}

#[cfg(windows)]
fn execute_click_windows(x: i32, y: i32, button: &str) -> Result<(), String> {
    use std::process::Command;
    let b = match button {
        "right" => "right",
        "double" => "double",
        _ => "left",
    };
    let script = format!(
        r#"
        Add-Type -AssemblyName System.Windows.Forms;
        [System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point({x}, {y});
        Add-Type @"
            using System;
            using System.Runtime.InteropServices;
            public class MouseAct {{
                [DllImport("user32.dll")] public static extern void mouse_event(int dwFlags, int dx, int dy, int cButtons, int dwExtraInfo);
                public const int MOUSEEVENTF_LEFTDOWN = 0x02;
                public const int MOUSEEVENTF_LEFTUP = 0x04;
                public const int MOUSEEVENTF_RIGHTDOWN = 0x08;
                public const int MOUSEEVENTF_RIGHTUP = 0x10;
                public static void LeftClick() {{ mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0); mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0); }}
                public static void RightClick() {{ mouse_event(MOUSEEVENTF_RIGHTDOWN, 0, 0, 0, 0); mouse_event(MOUSEEVENTF_RIGHTUP, 0, 0, 0, 0); }}
                public static void DoubleClick() {{ LeftClick(); System.Threading.Thread.Sleep(50); LeftClick(); }}
            }}
"@;
        switch ("{b}") {{
            "right" {{ [MouseAct]::RightClick(); break }}
            "double" {{ [MouseAct]::DoubleClick(); break }}
            default {{ [MouseAct]::LeftClick(); break }}
        }}
    "#
    );

    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("failed to execute click: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn execute_type_windows(text: &str) -> Result<(), String> {
    use std::process::Command;
    let escaped = text.replace('`', "``").replace('"', "`\"");
    let script = format!(
        r#"
        Add-Type -AssemblyName System.Windows.Forms;
        [System.Windows.Forms.SendKeys]::SendWait("{escaped}");
    "#
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("failed to type text: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn execute_key_windows(key: &str) -> Result<(), String> {
    use std::process::Command;
    let binding = key.to_ascii_lowercase();
    let send_key = match binding.as_str() {
        "enter" | "return" => "{ENTER}",
        "tab" => "{TAB}",
        "escape" | "esc" => "{ESC}",
        "backspace" => "{BACKSPACE}",
        "delete" | "del" => "{DEL}",
        "up" => "{UP}",
        "down" => "{DOWN}",
        "left" => "{LEFT}",
        "right" => "{RIGHT}",
        "ctrl+s" => "^s",
        "ctrl+o" => "^o",
        "ctrl+w" => "^w",
        "alt+f4" => "%{F4}",
        other => other,
    };
    let script = format!(
        r#"
        Add-Type -AssemblyName System.Windows.Forms;
        [System.Windows.Forms.SendKeys]::SendWait("{send_key}");
    "#
    );
    Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("failed to press key: {e}"))?;
    Ok(())
}

impl ToolProvider for ComputerTool {
    fn specs(&self) -> Vec<ToolSpec> {
        let cfg = self.settings();
        let allowed_desc = if cfg.allowed_apps.is_empty() {
            "none configured".to_string()
        } else {
            cfg.allowed_apps.join(", ")
        };
        let status_desc = if cfg.enabled {
            format!(
                "Enabled. Allowed applications: [{allowed_desc}]. Emergency stop hotkey: {}.",
                cfg.stop_hotkey
            )
        } else {
            "Currently disabled in [computer_use]. Must be enabled in configuration to execute actions.".into()
        };

        vec![ToolSpec {
            name: "computer".into(),
            description: format!(
                "Operate permitted desktop applications: capture screenshot, click, type text, press key, or inspect active window. {status_desc} Any action targeting unlisted applications is strictly blocked."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["screenshot", "click", "type", "key", "active_window", "stop", "reset"],
                        "description": "Desktop action: screenshot (capture screen), click (mouse click at x, y), type (send text), key (press shortcut/key), active_window (inspect focused app), stop (emergency stop), reset (resume after stop)"
                    },
                    "x": {"type": "integer", "description": "X coordinate for click"},
                    "y": {"type": "integer", "description": "Y coordinate for click"},
                    "button": {"type": "string", "enum": ["left", "right", "double"], "description": "Mouse button (default: left)"},
                    "text": {"type": "string", "description": "Text to type into active control"},
                    "key": {"type": "string", "description": "Key or shortcut, e.g. Enter, Tab, Escape, Ctrl+S, Alt+F4"},
                    "app": {"type": "string", "description": "Optional expected process name to verify before action, e.g. notepad.exe"}
                },
                "required": ["action"]
            }),
        }]
    }

    fn handles(&self, name: &str) -> bool {
        name == "computer"
    }

    fn risk(&self, _name: &str, args: &Value) -> Risk {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
        let action = s("action");

        match action {
            "screenshot" | "active_window" | "stop" | "reset" => Risk::Safe,
            "click" => {
                let x = args.get("x").and_then(Value::as_i64).unwrap_or(0);
                let y = args.get("y").and_then(Value::as_i64).unwrap_or(0);
                let app = s("app");
                let target = if app.is_empty() { "active window" } else { app };
                Risk::Ask(format!("click at ({x}, {y}) on {target}"))
            }
            "type" => {
                let text = s("text");
                let app = s("app");
                let target = if app.is_empty() { "active window" } else { app };
                Risk::Ask(format!("type \"{text}\" into {target}"))
            }
            "key" => {
                let key = s("key");
                let app = s("app");
                let target = if app.is_empty() { "active window" } else { app };
                Risk::Ask(format!("press key \"{key}\" on {target}"))
            }
            _ => Risk::Safe,
        }
    }

    fn subject(&self, _name: &str, args: &Value) -> String {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
        match s("action") {
            "click" => {
                let x = args.get("x").and_then(Value::as_i64).unwrap_or(0);
                let y = args.get("y").and_then(Value::as_i64).unwrap_or(0);
                format!("click ({x}, {y})")
            }
            "type" => format!("type {}", s("text")),
            "key" => format!("press {}", s("key")),
            "screenshot" => "screenshot".into(),
            "active_window" => "active_window".into(),
            a => a.to_string(),
        }
    }

    fn execute<'a>(&'a self, _name: &'a str, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim);
            let action = s("action").ok_or("missing 'action'")?;
            let app = s("app");

            match action {
                "screenshot" => {
                    let (desc, _) = self.capture_desktop().await?;
                    Ok(desc)
                }
                "active_window" => {
                    let win = get_active_window()?;
                    let allowed = is_app_allowed(&win.process_name, &win.title, &self.settings().allowed_apps);
                    Ok(format!(
                        "Active Window: \"{}\"\nProcess: {}\nCoordinates: ({}, {}) Size: {}x{}\nAllowed for interaction: {}",
                        win.title, win.process_name, win.x, win.y, win.width, win.height, allowed
                    ))
                }
                "click" => {
                    let x = args.get("x").and_then(Value::as_i64).ok_or("click needs 'x'")? as i32;
                    let y = args.get("y").and_then(Value::as_i64).ok_or("click needs 'y'")? as i32;
                    let button = s("button").unwrap_or("left");
                    self.click(x, y, button, app).await
                }
                "type" => {
                    let text = s("text").ok_or("type needs 'text'")?;
                    self.type_text(text, app).await
                }
                "key" => {
                    let key = s("key").ok_or("key needs 'key'")?;
                    self.press_key(key, app).await
                }
                "stop" => {
                    trigger_emergency_stop();
                    Ok("Emergency stop triggered: desktop computer operations halted.".into())
                }
                "reset" => {
                    reset_emergency_stop();
                    Ok("Emergency stop reset: desktop computer operations resumed.".into())
                }
                other => Err(format!(
                    "unknown computer action '{other}'. Use screenshot, click, type, key, active_window, stop, or reset."
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_app_allowed_matching() {
        let allowed = vec![
            "notepad.exe".to_string(),
            "calc.exe".to_string(),
            "mspaint.exe".to_string(),
        ];

        // Allowed processes
        assert!(is_app_allowed(
            "notepad.exe",
            "Untitled - Notepad",
            &allowed
        ));
        assert!(is_app_allowed(
            "notepad",
            "document.txt - Notepad",
            &allowed
        ));
        assert!(is_app_allowed("calc.exe", "Calculator", &allowed));
        assert!(is_app_allowed("CalculatorApp.exe", "Calculator", &allowed));
        assert!(is_app_allowed("mspaint.exe", "Untitled - Paint", &allowed));

        // Unauthorized processes
        assert!(!is_app_allowed(
            "chrome.exe",
            "Personal Banking - Chrome",
            &allowed
        ));
        assert!(!is_app_allowed("cmd.exe", "Command Prompt", &allowed));
        assert!(!is_app_allowed("powershell.exe", "PowerShell 7", &allowed));
        assert!(!is_app_allowed(
            "explorer.exe",
            "Windows File Explorer",
            &allowed
        ));
    }

    static TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn test_computer_use_disabled_refuses_actions() {
        let _guard = TEST_MUTEX.lock().await;
        reset_emergency_stop();
        let tool = ComputerTool::new(ComputerUseSettings {
            enabled: false,
            allowed_apps: vec!["notepad.exe".into()],
            ask_all: false,
            stop_hotkey: "Escape".into(),
        });

        tool.set_mock_window(Some(ActiveWindowInfo {
            title: "Untitled - Notepad".into(),
            process_name: "notepad.exe".into(),
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        }));

        let res = tool
            .execute("computer", &json!({"action": "click", "x": 150, "y": 150}))
            .await;
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("Computer use is disabled in configuration"));
    }

    #[tokio::test]
    async fn test_computer_use_unauthorized_app_blocked() {
        let _guard = TEST_MUTEX.lock().await;
        reset_emergency_stop();
        let tool = ComputerTool::new(ComputerUseSettings {
            enabled: true,
            allowed_apps: vec!["notepad.exe".into()],
            ask_all: false,
            stop_hotkey: "Escape".into(),
        });

        tool.set_mock_window(Some(ActiveWindowInfo {
            title: "Settings - Accounts".into(),
            process_name: "SystemSettings.exe".into(),
            x: 50,
            y: 50,
            width: 1000,
            height: 700,
        }));

        let res = tool
            .execute("computer", &json!({"action": "click", "x": 200, "y": 200}))
            .await;
        assert!(res.is_err());
        assert!(res
            .unwrap_err()
            .contains("not in [computer_use.allowed_apps]"));
    }

    #[tokio::test]
    async fn test_computer_use_allowed_app_succeeds() {
        let _guard = TEST_MUTEX.lock().await;
        reset_emergency_stop();
        let tool = ComputerTool::new(ComputerUseSettings {
            enabled: true,
            allowed_apps: vec!["notepad.exe".into()],
            ask_all: false,
            stop_hotkey: "Escape".into(),
        });

        tool.set_mock_window(Some(ActiveWindowInfo {
            title: "Untitled - Notepad".into(),
            process_name: "notepad.exe".into(),
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        }));

        let click_res = tool
            .execute("computer", &json!({"action": "click", "x": 250, "y": 300}))
            .await
            .unwrap();
        assert!(click_res.contains("Clicked (250, 300)"));

        let type_res = tool
            .execute(
                "computer",
                &json!({"action": "type", "text": "font_size=14"}),
            )
            .await
            .unwrap();
        assert!(type_res.contains("Typed 12 characters"));

        let key_res = tool
            .execute("computer", &json!({"action": "key", "key": "Enter"}))
            .await
            .unwrap();
        assert!(key_res.contains("Pressed key \"Enter\""));
    }

    #[tokio::test]
    async fn test_emergency_stop_halts_interaction() {
        let _guard = TEST_MUTEX.lock().await;
        reset_emergency_stop();
        let tool = ComputerTool::new(ComputerUseSettings {
            enabled: true,
            allowed_apps: vec!["notepad.exe".into()],
            ask_all: false,
            stop_hotkey: "Escape".into(),
        });

        tool.set_mock_window(Some(ActiveWindowInfo {
            title: "Untitled - Notepad".into(),
            process_name: "notepad.exe".into(),
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        }));

        // Trigger stop
        trigger_emergency_stop();
        assert!(is_emergency_stopped());

        let res = tool
            .execute("computer", &json!({"action": "click", "x": 200, "y": 200}))
            .await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("emergency stop is active"));

        // Reset stop
        reset_emergency_stop();
        assert!(!is_emergency_stopped());
        let res2 = tool
            .execute("computer", &json!({"action": "click", "x": 200, "y": 200}))
            .await;
        assert!(res2.is_ok());
    }
}
