//! Per-run trace file recording (Q6).
//!
//! Writes JSON lines (.jsonl) to `~/.oblivion/traces/<run_id>.jsonl` covering:
//! - Run start (timestamp, prompt, cwd)
//! - Model calls and fallbacks
//! - Tool invocations, durations, and outputs
//! - Approvals and user decisions
//! - Turn outcomes and token usages
//!
//! Enforces size caps on individual files and the traces folder.
//! `oblivion --trace <id>` reads and prints a trace file.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

pub const MAX_TRACE_FILES: usize = 50;
pub const MAX_TRACE_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEntry {
    Start {
        run_id: String,
        timestamp: String,
        prompt: String,
        cwd: String,
    },
    Model {
        timestamp: String,
        label: String,
    },
    Fallback {
        timestamp: String,
        from: String,
        reason: String,
    },
    ToolStart {
        timestamp: String,
        name: String,
        summary: String,
        detail: String,
    },
    ToolEnd {
        timestamp: String,
        name: String,
        duration_ms: u64,
        is_error: bool,
        preview: String,
    },
    Approval {
        timestamp: String,
        id: u64,
        tool: String,
        summary: String,
        decision: String,
    },
    TurnEnd {
        timestamp: String,
        duration_ms: u64,
        steps: usize,
        tool_calls: usize,
        prompt_tokens: u64,
        completion_tokens: u64,
        outcome: String,
        error: Option<String>,
    },
    Notice {
        timestamp: String,
        text: String,
    },
    Overflow {
        timestamp: String,
        message: String,
    },
}

pub struct Tracer {
    pub run_id: String,
    file_path: PathBuf,
    start_time: Instant,
    active_tool_start: Mutex<Option<(String, Instant)>>,
    file: Mutex<Option<File>>,
    bytes_written: Mutex<u64>,
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn generate_run_id() -> String {
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let mut rand_bytes = [0u8; 4];
    let _ = getrandom::fill(&mut rand_bytes);
    format!("{}-{}", stamp, hex::encode(rand_bytes))
}

mod hex {
    pub fn encode(bytes: [u8; 4]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl Tracer {
    /// Create a new tracer for a run in `traces_dir`.
    pub fn start(traces_dir: &Path, prompt: &str, cwd: &Path) -> Option<Self> {
        if let Err(e) = fs::create_dir_all(traces_dir) {
            eprintln!(
                "Warning: failed to create traces directory {}: {e}",
                traces_dir.display()
            );
            return None;
        }
        let _ = rotate_traces(traces_dir);

        let run_id = generate_run_id();
        let file_path = traces_dir.join(format!("{run_id}.jsonl"));

        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "Warning: cannot open trace file {}: {e}",
                    file_path.display()
                );
                return None;
            }
        };

        let tracer = Self {
            run_id: run_id.clone(),
            file_path,
            start_time: Instant::now(),
            active_tool_start: Mutex::new(None),
            file: Mutex::new(Some(file)),
            bytes_written: Mutex::new(0),
        };

        tracer.record(TraceEntry::Start {
            run_id,
            timestamp: now_iso(),
            prompt: prompt.to_string(),
            cwd: cwd.display().to_string(),
        });

        Some(tracer)
    }

    pub fn record(&self, entry: TraceEntry) {
        let mut file_guard = self.file.lock().unwrap();
        let Some(file) = file_guard.as_mut() else {
            return;
        };

        let mut bytes_guard = self.bytes_written.lock().unwrap();
        if *bytes_guard >= MAX_TRACE_BYTES {
            return;
        }

        if let Ok(line) = serde_json::to_string(&entry) {
            let data = format!("{line}\n");
            let len = data.len() as u64;
            if *bytes_guard + len > MAX_TRACE_BYTES {
                let overflow = TraceEntry::Overflow {
                    timestamp: now_iso(),
                    message: "Trace exceeded max size limit; logging stopped.".into(),
                };
                if let Ok(ov_line) = serde_json::to_string(&overflow) {
                    let _ = writeln!(file, "{ov_line}");
                }
                *bytes_guard = MAX_TRACE_BYTES;
                return;
            }
            if file.write_all(data.as_bytes()).is_ok() {
                let _ = file.flush();
                *bytes_guard += len;
            }
        }
    }

    pub fn record_model(&self, label: &str) {
        self.record(TraceEntry::Model {
            timestamp: now_iso(),
            label: label.to_string(),
        });
    }

    pub fn record_fallback(&self, from: &str, reason: &str) {
        self.record(TraceEntry::Fallback {
            timestamp: now_iso(),
            from: from.to_string(),
            reason: reason.to_string(),
        });
    }

    pub fn record_tool_start(&self, name: &str, summary: &str, detail: &str) {
        *self.active_tool_start.lock().unwrap() = Some((name.to_string(), Instant::now()));
        self.record(TraceEntry::ToolStart {
            timestamp: now_iso(),
            name: name.to_string(),
            summary: summary.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn record_tool_end(&self, name: &str, preview: &str, is_error: bool) {
        let duration_ms = match self.active_tool_start.lock().unwrap().take() {
            Some((tool, start)) if tool == name => start.elapsed().as_millis() as u64,
            _ => 0,
        };
        self.record(TraceEntry::ToolEnd {
            timestamp: now_iso(),
            name: name.to_string(),
            duration_ms,
            is_error,
            preview: preview.to_string(),
        });
    }

    pub fn record_declined(&self, name: &str) {
        self.record(TraceEntry::Approval {
            timestamp: now_iso(),
            id: 0,
            tool: name.to_string(),
            summary: "Execution declined by user".to_string(),
            decision: "deny".to_string(),
        });
    }

    pub fn record_approval(&self, id: u64, tool: &str, summary: &str, decision: &str) {
        self.record(TraceEntry::Approval {
            timestamp: now_iso(),
            id,
            tool: tool.to_string(),
            summary: summary.to_string(),
            decision: decision.to_string(),
        });
    }

    pub fn finish(
        &self,
        steps: usize,
        tool_calls: usize,
        prompt_tokens: u64,
        completion_tokens: u64,
        outcome: &str,
        error: Option<&str>,
    ) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;
        self.record(TraceEntry::TurnEnd {
            timestamp: now_iso(),
            duration_ms,
            steps,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            outcome: outcome.to_string(),
            error: error.map(String::from),
        });
    }

    pub fn file_path(&self) -> &Path {
        &self.file_path
    }
}

/// Cap the number of trace files kept in `traces_dir` to `MAX_TRACE_FILES`.
pub fn rotate_traces(traces_dir: &Path) -> Result<usize> {
    if !traces_dir.exists() {
        return Ok(0);
    }
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in fs::read_dir(traces_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            let mod_time = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            files.push((path, mod_time));
        }
    }

    // Sort newest first
    files.sort_by_key(|a| std::cmp::Reverse(a.1));

    let mut deleted = 0;
    if files.len() > MAX_TRACE_FILES {
        for (path, _) in &files[MAX_TRACE_FILES..] {
            if fs::remove_file(path).is_ok() {
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

/// `oblivion --trace <id>`: print a trace file or list available ones.
pub fn print_trace(traces_dir: &Path, id: &str) -> Result<()> {
    if !traces_dir.exists() {
        if id == "list" || id.trim().is_empty() {
            println!("No traces recorded yet in {}", traces_dir.display());
            return Ok(());
        }
        bail!(
            "Trace '{id}' not found (no trace directory at {}).",
            traces_dir.display()
        );
    }

    if id == "list" || id.trim().is_empty() {
        return list_traces(traces_dir);
    }

    let target_path = if id.ends_with(".jsonl") {
        traces_dir.join(id)
    } else {
        traces_dir.join(format!("{id}.jsonl"))
    };

    if !target_path.exists() {
        // Try finding a partial prefix match
        let mut matches = Vec::new();
        for entry in fs::read_dir(traces_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(id) && name.ends_with(".jsonl") {
                matches.push(entry.path());
            }
        }
        if matches.len() == 1 {
            return output_trace_file(&matches[0]);
        } else if matches.len() > 1 {
            bail!("Ambiguous trace ID '{id}'; matches multiple trace files.");
        } else {
            eprintln!("Trace '{id}' not found in {}.", traces_dir.display());
            eprintln!("\nAvailable traces:");
            let _ = list_traces(traces_dir);
            bail!("Trace not found.");
        }
    }

    output_trace_file(&target_path)
}

fn output_trace_file(path: &Path) -> Result<()> {
    let file = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            println!("{line}");
        }
    }
    Ok(())
}

fn list_traces(traces_dir: &Path) -> Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(traces_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let first_line = File::open(&path).ok().and_then(|f| {
                let mut reader = BufReader::new(f);
                let mut line = String::new();
                reader.read_line(&mut line).ok()?;
                serde_json::from_str::<TraceEntry>(&line).ok()
            });
            entries.push((stem, path, first_line));
        }
    }

    // Sort newest first by stem/date
    entries.sort_by_key(|a| std::cmp::Reverse(a.0.clone()));

    if entries.is_empty() {
        println!("No traces recorded yet in {}", traces_dir.display());
        return Ok(());
    }

    println!("{:<28} {:<24} PROMPT", "TRACE ID", "STARTED");
    println!("{}", "-".repeat(80));
    for (id, _, first) in entries.iter().take(25) {
        let (time, prompt) = match first {
            Some(TraceEntry::Start {
                timestamp, prompt, ..
            }) => {
                let p_short: String = if prompt.len() > 40 {
                    format!("{}…", &prompt[..39])
                } else {
                    prompt.clone()
                };
                (timestamp.clone(), p_short)
            }
            _ => ("-".to_string(), "-".to_string()),
        };
        println!("{:<28} {:<24} {}", id, time, prompt);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn trace_recording_and_roundtrip() {
        let dir = tempdir().unwrap();
        let traces_dir = dir.path().join("traces");

        let tracer = Tracer::start(&traces_dir, "Review codebase", dir.path()).unwrap();
        let run_id = tracer.run_id.clone();

        tracer.record_model("groq/llama-3.3-70b");
        tracer.record_tool_start("read_file", "read README.md", "README.md");
        tracer.record_tool_end("read_file", "# Title", false);
        tracer.record_approval(1, "edit_file", "edit src/main.rs", "allow");
        tracer.finish(2, 1, 100, 50, "done", None);

        let trace_path = traces_dir.join(format!("{run_id}.jsonl"));
        assert!(trace_path.exists());

        let lines: Vec<TraceEntry> = BufReader::new(File::open(&trace_path).unwrap())
            .lines()
            .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
            .collect();

        assert_eq!(lines.len(), 6);
        assert!(
            matches!(&lines[0], TraceEntry::Start { prompt, .. } if prompt == "Review codebase")
        );
        assert!(
            matches!(&lines[1], TraceEntry::Model { label, .. } if label == "groq/llama-3.3-70b")
        );
        assert!(matches!(&lines[2], TraceEntry::ToolStart { name, .. } if name == "read_file"));
        assert!(
            matches!(&lines[3], TraceEntry::ToolEnd { name, is_error: false, .. } if name == "read_file")
        );
        assert!(
            matches!(&lines[4], TraceEntry::Approval { tool, decision, .. } if tool == "edit_file" && decision == "allow")
        );
        assert!(
            matches!(&lines[5], TraceEntry::TurnEnd { steps: 2, tool_calls: 1, outcome, .. } if outcome == "done")
        );
    }

    #[test]
    fn trace_rotation_caps_file_count() {
        let dir = tempdir().unwrap();
        let traces_dir = dir.path().join("traces");
        fs::create_dir_all(&traces_dir).unwrap();

        // Create 55 fake trace files
        for i in 0..55 {
            let p = traces_dir.join(format!("trace_{i:03}.jsonl"));
            fs::write(p, "{\"type\":\"start\"}\n").unwrap();
        }

        let deleted = rotate_traces(&traces_dir).unwrap();
        assert_eq!(deleted, 5);

        let remaining: Vec<_> = fs::read_dir(&traces_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        assert_eq!(remaining.len(), MAX_TRACE_FILES);
    }

    #[test]
    fn trace_size_cap_stops_logging_and_writes_overflow() {
        let dir = tempdir().unwrap();
        let traces_dir = dir.path().join("traces");
        let tracer = Tracer::start(&traces_dir, "test", dir.path()).unwrap();
        let big = "x".repeat(1024 * 1024); // 1 MB
        for i in 0..6 {
            tracer.record_tool_start("tool", &format!("call {i}"), &big);
        }
        let trace_path = tracer.file_path();
        let metadata = fs::metadata(trace_path).unwrap();
        assert!(metadata.len() <= MAX_TRACE_BYTES + 500);
        let content = fs::read_to_string(trace_path).unwrap();
        assert!(content.contains("\"type\":\"overflow\""));
    }

    #[test]
    fn print_trace_and_list() {
        let dir = tempdir().unwrap();
        let traces_dir = dir.path().join("traces");
        let tracer = Tracer::start(&traces_dir, "find rust bugs", dir.path()).unwrap();
        let run_id = tracer.run_id.clone();
        tracer.finish(1, 0, 10, 5, "done", None);

        // List traces
        assert!(print_trace(&traces_dir, "list").is_ok());

        // Exact ID
        assert!(print_trace(&traces_dir, &run_id).is_ok());

        // Prefix ID
        let prefix = &run_id[..10];
        assert!(print_trace(&traces_dir, prefix).is_ok());

        // Unknown ID
        assert!(print_trace(&traces_dir, "non-existent-id").is_err());
    }
}
