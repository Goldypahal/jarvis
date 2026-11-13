//! Shell command tool with timeout and output capture (SRS FR-14).

use std::process::Stdio;
use std::time::Duration;

use oblivion_providers::ToolSpec;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::{str_arg, ToolBox};

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    PowerShell,
    Cmd,
    Bash,
    Sh,
}

impl Shell {
    pub fn platform_default() -> Self {
        if cfg!(windows) {
            Shell::PowerShell
        } else {
            Shell::Bash
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "powershell" | "pwsh" => Some(Shell::PowerShell),
            "cmd" => Some(Shell::Cmd),
            "bash" => Some(Shell::Bash),
            "sh" => Some(Shell::Sh),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Shell::PowerShell => "PowerShell",
            Shell::Cmd => "cmd.exe",
            Shell::Bash => "bash",
            Shell::Sh => "sh",
        }
    }

    fn command(self, script: &str) -> Command {
        let mut c = match self {
            Shell::PowerShell => {
                let mut c = Command::new("powershell");
                c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
                c
            }
            Shell::Cmd => {
                let mut c = Command::new("cmd");
                c.args(["/C", script]);
                c
            }
            Shell::Bash => {
                let mut c = Command::new("bash");
                c.args(["-c", script]);
                c
            }
            Shell::Sh => {
                let mut c = Command::new("sh");
                c.args(["-c", script]);
                c
            }
        };
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        c
    }
}

pub(crate) fn spec(shell: Shell) -> ToolSpec {
    ToolSpec {
        name: "shell".into(),
        description: format!(
            "Run a command in {} in the workspace folder and return its output and exit code. \
             Use it for builds, tests, git, searching files and installing packages. Commands \
             cannot read keyboard input; pass flags such as -y. Default timeout {DEFAULT_TIMEOUT_SECS}s.",
            shell.name()
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": format!("The {} command to run", shell.name())},
                "timeout_secs": {"type": "integer", "description": format!("Timeout in seconds (max {MAX_TIMEOUT_SECS})")}
            },
            "required": ["command"]
        }),
    }
}

pub(crate) async fn run(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let cmd = str_arg(args, "command")?;
    let timeout = args
        .get("timeout_secs")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    let mut command = tb.shell().command(cmd);
    command.current_dir(tb.root());
    let child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", tb.shell().name()))?;

    match tokio::time::timeout(Duration::from_secs(timeout), child.wait_with_output()).await {
        Err(_) => Err(format!(
            "command timed out after {timeout}s and was stopped"
        )),
        Ok(Err(e)) => Err(format!("command failed to run: {e}")),
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let code = out
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "killed".into());
            let mut text = format!("exit code: {code}\n");
            if !stdout.trim().is_empty() {
                text.push_str("--- stdout ---\n");
                text.push_str(stdout.trim_end());
                text.push('\n');
            }
            if !stderr.trim().is_empty() {
                text.push_str("--- stderr ---\n");
                text.push_str(stderr.trim_end());
                text.push('\n');
            }
            Ok(text)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use crate::{Shell, ToolBox};
    use serde_json::json;

    #[tokio::test]
    async fn runs_and_times_out() {
        let d = tempfile::tempdir().unwrap();
        let t = ToolBox::new(d.path(), Shell::Sh, d.path().join(".snap"));
        let o = t
            .execute(
                "shell",
                &json!({"command":"echo hi; echo oops 1>&2; exit 3"}),
            )
            .await;
        assert!(
            o.text.contains("exit code: 3") && o.text.contains("hi") && o.text.contains("oops"),
            "{}",
            o.text
        );
        let o = t
            .execute("shell", &json!({"command":"sleep 5","timeout_secs":1}))
            .await;
        assert!(o.is_error && o.text.contains("timed out"));
    }
}
