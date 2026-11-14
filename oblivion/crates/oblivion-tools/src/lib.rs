//! Built-in tools (SRS FR-10..FR-17): files, search, shell, web and todo.
//!
//! The agent core asks [`ToolBox::risk`] whether a call needs the user's
//! approval, then runs it with [`ToolBox::execute`]. Every file change is
//! snapshotted first so it can be undone (SRS FR-62).

mod fs_tools;
mod fuzzy;
mod search;
mod shell;
mod snapshot;
mod todo;
mod web;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use oblivion_providers::ToolSpec;
use serde_json::Value;

pub use shell::Shell;
pub use snapshot::{Journal, UndoReport};
pub use todo::{TodoItem, TodoList, TodoStatus};
pub use web::{html_to_text, parse_duckduckgo, SearchBackend, SearchResult};

/// Result of running a tool, returned to the model as the tool message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
        }
    }
    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: format!("Error: {}", text.into()),
            is_error: true,
        }
    }
}

/// Whether a tool call may run without asking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Risk {
    /// Read-only, inside the workspace.
    Safe,
    /// Changes something; the string says what, for the approval prompt.
    Ask(String),
}

/// A boxed future, so tool providers can be stored as trait objects.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;

/// An extra set of tools plugged into the ToolBox (skills, MCP servers…).
pub trait ToolProvider: Send + Sync {
    fn specs(&self) -> Vec<ToolSpec>;
    fn handles(&self, name: &str) -> bool;
    /// Defaults to asking, the safe choice for tools Oblivion did not write.
    fn risk(&self, name: &str, _args: &Value) -> Risk {
        Risk::Ask(format!("use {name}"))
    }
    /// Text that permission rules are matched against.
    fn subject(&self, _name: &str, _args: &Value) -> String {
        String::new()
    }
    fn execute<'a>(&'a self, name: &'a str, args: &'a Value) -> ToolFuture<'a>;
}

/// Everything tools need to know about where they run.
pub struct ToolBox {
    root: PathBuf,
    shell: Shell,
    journal: Journal,
    max_output_chars: usize,
    todos: TodoList,
    http: reqwest::Client,
    search: SearchBackend,
    providers: Vec<Arc<dyn ToolProvider>>,
}

impl ToolBox {
    pub fn new(root: impl Into<PathBuf>, shell: Shell, snapshot_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            shell,
            journal: Journal::new(snapshot_dir.into()),
            max_output_chars: 30_000,
            todos: TodoList::default(),
            http: web::http_client(),
            search: SearchBackend::DuckDuckGo,
            providers: Vec::new(),
        }
    }

    pub fn add_provider(&mut self, p: Arc<dyn ToolProvider>) {
        self.providers.push(p);
    }

    fn provider_for(&self, name: &str) -> Option<&Arc<dyn ToolProvider>> {
        self.providers.iter().find(|p| p.handles(name))
    }

    pub fn with_search(mut self, backend: SearchBackend) -> Self {
        self.search = backend;
        self
    }

    pub fn todos(&self) -> &TodoList {
        &self.todos
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub(crate) fn search_backend(&self) -> &SearchBackend {
        &self.search
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn shell(&self) -> Shell {
        self.shell
    }

    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = fs_tools::specs();
        specs.extend(search::specs());
        specs.push(shell::spec(self.shell));
        specs.extend(web::specs());
        specs.push(todo::spec());
        for p in &self.providers {
            specs.extend(p.specs());
        }
        specs
    }

    /// Resolve a model-supplied path against the workspace root.
    pub fn resolve(&self, raw: &str) -> PathBuf {
        let p = Path::new(raw.trim());
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    pub fn inside_root(&self, path: &Path) -> bool {
        path_starts_with(&normalize_path(path), &normalize_path(&self.root))
    }

    pub fn risk(&self, name: &str, args: &Value) -> Risk {
        if let Some(p) = self.provider_for(name) {
            return p.risk(name, args);
        }
        let path_arg = || args.get("path").and_then(Value::as_str).unwrap_or("");
        match name {
            "read_file" => {
                let p = self.resolve(path_arg());
                if self.inside_root(&p) {
                    Risk::Safe
                } else {
                    Risk::Ask(format!("read {} (outside the workspace)", p.display()))
                }
            }
            "write_file" => Risk::Ask(format!("write {}", self.resolve(path_arg()).display())),
            "edit_file" => Risk::Ask(format!("edit {}", self.resolve(path_arg()).display())),
            "glob" | "grep" => match args.get("path").and_then(Value::as_str) {
                Some(p) if !self.inside_root(&self.resolve(p)) => Risk::Ask(format!(
                    "search {} (outside the workspace)",
                    self.resolve(p).display()
                )),
                _ => Risk::Safe,
            },
            "shell" => {
                let cmd = args.get("command").and_then(Value::as_str).unwrap_or("");
                Risk::Ask(format!("run: {cmd}"))
            }
            "web_fetch" => {
                let url = args.get("url").and_then(Value::as_str).unwrap_or("");
                Risk::Ask(format!("fetch {url}"))
            }
            _ => Risk::Safe,
        }
    }

    /// The text permission rules are matched against: the command for `shell`,
    /// the URL for `web_fetch`, the query for `web_search`, otherwise the path.
    pub fn subject(&self, name: &str, args: &Value) -> String {
        let get = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        if let Some(p) = self.provider_for(name) {
            return p.subject(name, args);
        }
        match name {
            "shell" => get("command"),
            "web_fetch" => get("url"),
            "web_search" => get("query"),
            "todo" => String::new(),
            _ => {
                let p = get("path");
                if p.is_empty() {
                    ".".into()
                } else {
                    p.replace('\\', "/")
                }
            }
        }
    }

    pub async fn execute(&self, name: &str, args: &Value) -> ToolOutput {
        let out = match name {
            "read_file" => fs_tools::read_file(self, args).await,
            "write_file" => fs_tools::write_file(self, args).await,
            "edit_file" => fs_tools::edit_file(self, args).await,
            "glob" => search::glob(self, args).await,
            "grep" => search::grep(self, args).await,
            "shell" => shell::run(self, args).await,
            "web_search" => web::web_search(self, args).await,
            "web_fetch" => web::web_fetch(self, args).await,
            "todo" => todo::update(&self.todos, args),
            other if self.provider_for(other).is_some() => {
                self.provider_for(other)
                    .expect("checked")
                    .execute(other, args)
                    .await
            }
            other => Err(format!(
                "unknown tool '{other}'. Available tools: {}",
                self.specs()
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        };
        match out {
            Ok(text) => ToolOutput::ok(truncate_middle(&text, self.max_output_chars)),
            Err(e) => ToolOutput::err(e),
        }
    }
}

/// Keep the head and tail of long output; models need both ends of logs.
pub fn truncate_middle(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let half = max_chars / 2;
    let head: String = s.chars().take(half).collect();
    let tail: String = s.chars().skip(total - half).collect();
    format!(
        "{head}\n\n[... {} characters omitted ...]\n\n{tail}",
        total - 2 * half
    )
}

/// Absolute, symlink-resolved form of `p` that also works for files that do
/// not exist yet: the longest existing ancestor is canonicalized and the rest
/// is appended. Windows `\\?\` prefixes are removed so paths compare equal.
pub fn normalize_path(p: &Path) -> PathBuf {
    let clean = lexical_normalize(p);
    for ancestor in clean.ancestors() {
        if let Ok(canon) = ancestor.canonicalize() {
            let rest = clean.strip_prefix(ancestor).unwrap_or(Path::new(""));
            let base = PathBuf::from(strip_verbatim(&canon.to_string_lossy()));
            return if rest.as_os_str().is_empty() {
                base
            } else {
                base.join(rest)
            };
        }
    }
    PathBuf::from(strip_verbatim(&clean.to_string_lossy()))
}

/// `\\?\C:\x` → `C:\x`, `\\?\UNC\server\share` → `\\server\share`; other paths unchanged.
pub fn strip_verbatim(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        let b = rest.as_bytes();
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            rest.to_string()
        } else {
            s.to_string()
        }
    } else {
        s.to_string()
    }
}

/// Component-wise prefix test; case-insensitive on Windows, where paths are.
fn path_starts_with(path: &Path, base: &Path) -> bool {
    if cfg!(windows) {
        let lower = |p: &Path| PathBuf::from(p.to_string_lossy().to_lowercase());
        lower(path).starts_with(lower(base))
    } else {
        path.starts_with(base)
    }
}

fn lexical_normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub(crate) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string argument '{key}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_in_the_middle() {
        let s = "a".repeat(50) + &"b".repeat(50);
        let t = truncate_middle(&s, 20);
        assert!(t.starts_with("aaaaaaaaaa"));
        assert!(t.ends_with("bbbbbbbbbb"));
        assert!(t.contains("80 characters omitted"));
    }

    #[test]
    fn risk_levels() {
        let dir = tempfile::tempdir().unwrap();
        let tb = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
        assert_eq!(
            tb.risk("read_file", &serde_json::json!({"path":"a.txt"})),
            Risk::Safe
        );
        assert!(matches!(
            tb.risk("read_file", &serde_json::json!({"path":"../../etc/passwd"})),
            Risk::Ask(_)
        ));
        assert!(matches!(
            tb.risk("shell", &serde_json::json!({"command":"ls"})),
            Risk::Ask(_)
        ));
    }

    #[test]
    fn new_files_in_new_folders_are_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let tb = ToolBox::new(dir.path(), Shell::Sh, dir.path().join(".snap"));
        assert!(tb.inside_root(&tb.resolve("not/yet/created/file.rs")));
        assert!(tb.inside_root(&tb.resolve("a/../b.txt")));
        assert!(!tb.inside_root(&tb.resolve("../escape.txt")));
        assert!(!tb.inside_root(&tb.resolve("x/../../escape.txt")));
    }

    #[test]
    fn strips_windows_verbatim_prefixes() {
        assert_eq!(strip_verbatim(r"\\?\C:\Users\goldy\x"), r"C:\Users\goldy\x");
        assert_eq!(
            strip_verbatim(r"\\?\UNC\server\share\x"),
            r"\\server\share\x"
        );
        assert_eq!(strip_verbatim(r"\\?\Volume{abc}\x"), r"\\?\Volume{abc}\x");
        assert_eq!(strip_verbatim("/home/me"), "/home/me");
    }
}
