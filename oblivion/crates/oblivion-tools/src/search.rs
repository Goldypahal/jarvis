//! File search tools (SRS FR-13): `glob` finds files by name, `grep` finds
//! text inside files. Both respect .gitignore and skip hidden folders.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use oblivion_providers::ToolSpec;
use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::{str_arg, ToolBox};

const MAX_GLOB_RESULTS: usize = 200;
const MAX_GREP_LINES: usize = 200;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

pub(crate) fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "glob".into(),
            description: "Find files by name pattern, e.g. \"**/*.rs\", \"src/**/test_*.py\" or \"*.md\". \
                A pattern without '/' matches file names in any folder. Respects .gitignore. \
                Newest files first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern"},
                    "path": {"type": "string", "description": "Folder to search (default: workspace)"}
                },
                "required": ["pattern"]
            }),
        },
        ToolSpec {
            name: "grep".into(),
            description: "Search file contents with a regular expression. Returns matching lines as \
                path:line:text. Use `glob` to limit file types (e.g. \"*.py\"). Respects .gitignore."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regular expression (Rust regex syntax)"},
                    "path": {"type": "string", "description": "File or folder to search (default: workspace)"},
                    "glob": {"type": "string", "description": "Only search files matching this glob"},
                    "case_insensitive": {"type": "boolean"},
                    "files_only": {"type": "boolean", "description": "Only list matching file paths"}
                },
                "required": ["pattern"]
            }),
        },
    ]
}

fn matcher(pattern: &str) -> Result<GlobMatcher, String> {
    // "*.rs" should match in every folder, like most users expect.
    let full = if pattern.contains('/') || pattern.starts_with("**") {
        pattern.to_string()
    } else {
        format!("**/{pattern}")
    };
    GlobBuilder::new(&full)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| format!("invalid glob pattern '{pattern}': {e}"))
}

/// Path relative to `base`, always with forward slashes so globs behave the same on Windows.
fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walker(base: &Path) -> ignore::Walk {
    WalkBuilder::new(base)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .require_git(false)
        .build()
}

pub(crate) async fn glob(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let pattern = str_arg(args, "pattern")?.to_string();
    let base = match args.get("path").and_then(Value::as_str) {
        Some(p) => tb.resolve(p),
        None => tb.root().to_path_buf(),
    };
    let m = matcher(&pattern)?;
    let display_base = tb.root().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
        for entry in walker(&base).flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            if m.is_match(rel(&base, entry.path())) {
                let mtime = entry
                    .metadata()
                    .ok()
                    .and_then(|md| md.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                found.push((mtime, entry.into_path()));
            }
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.0));
        if found.is_empty() {
            return Ok(format!("No files match '{pattern}'"));
        }
        let total = found.len();
        let mut out: Vec<String> = found
            .into_iter()
            .take(MAX_GLOB_RESULTS)
            .map(|(_, p)| rel(&display_base, &p))
            .collect();
        if total > MAX_GLOB_RESULTS {
            out.push(format!(
                "[{} more files not shown; narrow the pattern]",
                total - MAX_GLOB_RESULTS
            ));
        }
        Ok(out.join("\n"))
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) async fn grep(tb: &ToolBox, args: &Value) -> Result<String, String> {
    let pattern = str_arg(args, "pattern")?;
    let re = RegexBuilder::new(pattern)
        .case_insensitive(
            args.get("case_insensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )
        .build()
        .map_err(|e| format!("invalid regex: {e}"))?;
    let base = match args.get("path").and_then(Value::as_str) {
        Some(p) => tb.resolve(p),
        None => tb.root().to_path_buf(),
    };
    let filter = match args.get("glob").and_then(Value::as_str) {
        Some(g) => Some(matcher(g)?),
        None => None,
    };
    let files_only = args
        .get("files_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let display_base = tb.root().to_path_buf();

    tokio::task::spawn_blocking(move || {
        let mut lines = Vec::new();
        let mut truncated = false;
        'walk: for entry in walker(&base).flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            if let Some(f) = &filter {
                if !f.is_match(rel(&base, path)) {
                    continue;
                }
            }
            if entry
                .metadata()
                .map(|m| m.len() > MAX_FILE_BYTES)
                .unwrap_or(true)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            if bytes.iter().take(8192).any(|b| *b == 0) {
                continue; // binary
            }
            let text = String::from_utf8_lossy(&bytes);
            let shown = rel(&display_base, path);
            let mut hit = false;
            for (n, line) in text.lines().enumerate() {
                if re.is_match(line) {
                    hit = true;
                    if files_only {
                        break;
                    }
                    if lines.len() >= MAX_GREP_LINES {
                        truncated = true;
                        break 'walk;
                    }
                    let line: String = line.chars().take(300).collect();
                    lines.push(format!("{shown}:{}:{}", n + 1, line.trim_end()));
                }
            }
            if hit && files_only {
                lines.push(shown);
                if lines.len() >= MAX_GREP_LINES {
                    truncated = true;
                    break;
                }
            }
        }
        if lines.is_empty() {
            return Ok(format!("No matches for /{}/", re.as_str()));
        }
        if truncated {
            lines.push(format!(
                "[stopped at {MAX_GREP_LINES} results; narrow the pattern or path]"
            ));
        }
        Ok(lines.join("\n"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use crate::{Shell, ToolBox};
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, ToolBox) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        std::fs::create_dir_all(p.join("src/inner")).unwrap();
        std::fs::create_dir_all(p.join("target")).unwrap();
        std::fs::write(p.join("src/main.rs"), "fn main() {\n    hello();\n}\n").unwrap();
        std::fs::write(p.join("src/inner/lib.rs"), "pub fn Hello() {}\n").unwrap();
        std::fs::write(p.join("README.md"), "hello docs\n").unwrap();
        std::fs::write(p.join("target/junk.rs"), "fn hello() {}\n").unwrap();
        std::fs::write(p.join(".gitignore"), "target/\n").unwrap();
        let tb = ToolBox::new(p, Shell::Sh, p.join(".snap"));
        (d, tb)
    }

    #[tokio::test]
    async fn glob_respects_gitignore_and_bare_patterns() {
        let (_d, tb) = setup();
        let o = tb.execute("glob", &json!({"pattern":"*.rs"})).await;
        assert!(
            o.text.contains("src/main.rs") && o.text.contains("src/inner/lib.rs"),
            "{}",
            o.text
        );
        assert!(
            !o.text.contains("junk.rs"),
            "gitignored file listed: {}",
            o.text
        );
        let o = tb.execute("glob", &json!({"pattern":"src/*.rs"})).await;
        assert!(
            o.text.contains("src/main.rs") && !o.text.contains("lib.rs"),
            "{}",
            o.text
        );
    }

    #[tokio::test]
    async fn grep_content_case_and_filters() {
        let (_d, tb) = setup();
        let o = tb.execute("grep", &json!({"pattern":"hello"})).await;
        assert!(o.text.contains("src/main.rs:2:    hello();"), "{}", o.text);
        assert!(o.text.contains("README.md:1:hello docs"));
        assert!(!o.text.contains("lib.rs") && !o.text.contains("junk"));
        let o = tb
            .execute(
                "grep",
                &json!({"pattern":"hello","case_insensitive":true,"glob":"*.rs","files_only":true}),
            )
            .await;
        assert_eq!(o.text.lines().count(), 2, "{}", o.text);
        assert!(!o.text.contains("README"));
        let o = tb.execute("grep", &json!({"pattern":"("})).await;
        assert!(o.is_error && o.text.contains("invalid regex"));
    }
}
