//! The `browser` tool: a real web browser, driven through Microsoft's Playwright
//! MCP server (`npx @playwright/mcp`).
//!
//! Why a dedicated tool instead of the generic `mcp` one:
//! - one compact schema, no "list the tools first" round trip;
//! - page outlines are trimmed (filler lines and generated code removed) and
//!   only the first part is shown, with `find` to search the rest locally for free;
//! - risk is judged from the element's real label in the last page outline, so
//!   clicking "Submit", "Pay" or "Delete" asks the user, while reading pages,
//!   typing and choosing answers runs freely.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use oblivion_providers::ToolSpec;
use oblivion_tools::{Risk, ToolFuture, ToolProvider};
use serde_json::{json, Value};

use crate::{McpHub, ServerConfig, Transport};

const SERVER: &str = "browser";
/// Characters of a page outline shown to the model per action (about 2.5K tokens).
pub const VIEW_CHARS: usize = 10_000;
/// Largest page outline kept for `find`.
const MAX_PAGE_CHARS: usize = 400_000;
const FIND_CONTEXT_LINES: usize = 8;
const FIND_MAX_LINES: usize = 80;

/// Words on a button or link that mean "this does something final".
const FINAL_WORDS: &[&str] = &[
    "submit",
    "send",
    "pay",
    "payment",
    "buy",
    "purchase",
    "order",
    "checkout",
    "confirm",
    "delete",
    "remove",
    "publish",
    "post",
    "register",
    "subscribe",
    "unsubscribe",
    "transfer",
    "donate",
    "book",
    "reserve",
    "finish",
    "apply",
    "save",
    "upload",
    "share",
    "accept",
    "agree",
];
const FINAL_PAIRS: &[(&str, &str)] = &[("sign", "up"), ("check", "out"), ("place", "order")];
/// Outline roles that carry no information on their own (their children stay).
const FILLER_ROLES: &[&str] = &[
    "generic",
    "group",
    "none",
    "presentation",
    "list",
    "listitem",
    "rowgroup",
    "main",
    "banner",
    "contentinfo",
    "navigation",
    "complementary",
    "article",
    "section",
    "region",
    "document",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserMode {
    /// A separate browser window with its own saved profile (logins persist).
    Own,
    /// The user's own Chrome/Edge tabs, through the Playwright MCP Bridge extension.
    Extension,
}

#[derive(Debug, Clone)]
pub struct BrowserSettings {
    pub mode: BrowserMode,
    /// chrome (default), msedge, firefox or webkit.
    pub browser: Option<String>,
    pub headless: bool,
    /// Profile folder for `Own` mode, so logins are kept between runs.
    pub profile_dir: Option<PathBuf>,
    pub command: String,
    pub package: String,
    pub timeout: Duration,
}

impl BrowserSettings {
    pub fn server_args(&self) -> Vec<String> {
        let mut a = vec!["-y".to_string(), self.package.clone()];
        match self.mode {
            BrowserMode::Extension => a.push("--extension".into()),
            BrowserMode::Own => {
                if let Some(d) = &self.profile_dir {
                    a.push(format!("--user-data-dir={}", d.display()));
                }
                if self.headless {
                    a.push("--headless".into());
                }
            }
        }
        if let Some(b) = self.browser.as_deref().filter(|b| !b.trim().is_empty()) {
            a.push(format!("--browser={}", b.trim()));
        }
        a
    }
}

#[derive(Default)]
struct PageState {
    /// Latest page outline (trimmed), for `find` and for judging risk.
    page: String,
    url: String,
    /// Argument name for element refs: "target" (newer servers) or "ref" (older).
    ref_param: Option<String>,
}

pub struct BrowserTool {
    hub: McpHub,
    state: StdMutex<PageState>,
}

impl BrowserTool {
    pub fn new(settings: &BrowserSettings) -> Self {
        Self::with_server(ServerConfig {
            name: SERVER.into(),
            description: Some("web browser".into()),
            transport: Transport::Stdio {
                command: settings.command.clone(),
                args: settings.server_args(),
                env: BTreeMap::new(),
            },
            timeout: settings.timeout,
        })
    }

    /// A browser served by any MCP server that speaks Playwright MCP's tools.
    pub fn with_server(mut config: ServerConfig) -> Self {
        config.name = SERVER.into();
        Self {
            hub: McpHub::new(vec![config]),
            state: StdMutex::new(PageState::default()),
        }
    }

    pub async fn shutdown(&self) {
        self.hub.shutdown().await;
    }

    /// Start the server and make sure it is Playwright MCP (for `--setup-browser`).
    pub async fn check(&self) -> Result<(), String> {
        self.ref_param().await.map(|_| ())
    }

    /// Open a page and return what the model would see (for `--setup-browser`).
    pub async fn try_open(&self, url: &str) -> Result<String, String> {
        self.run(&json!({"action": "open", "url": url})).await
    }

    async fn ref_param(&self) -> Result<String, String> {
        if let Some(p) = self.state.lock().unwrap().ref_param.clone() {
            return Ok(p);
        }
        let params = self.hub.tool_params(SERVER, "browser_click").await?;
        if params.is_empty() {
            return Err(
                "the browser server has no browser_click tool; is it Playwright MCP?".into(),
            );
        }
        let p = if params.iter().any(|p| p == "target") {
            "target"
        } else {
            "ref"
        }
        .to_string();
        self.state.lock().unwrap().ref_param = Some(p.clone());
        Ok(p)
    }

    fn label_of(&self, r: &str) -> Option<String> {
        ref_label(&self.state.lock().unwrap().page, r)
    }

    fn url(&self) -> String {
        self.state.lock().unwrap().url.clone()
    }

    /// Keep the trimmed outline for `find` and return the part the model sees.
    fn absorb(&self, raw: &str) -> String {
        let text = compact_response(raw);
        if text.contains("[ref=") {
            let mut st = self.state.lock().unwrap();
            st.page = text.clone();
            if let Some(u) = page_url(&text) {
                st.url = u;
            }
        }
        view(&text, VIEW_CHARS)
    }

    fn find(&self, needle: &str) -> String {
        let st = self.state.lock().unwrap();
        if st.page.is_empty() {
            return "No page has been loaded yet. Use action=open first.".into();
        }
        find_in_page(&st.page, &st.url, needle)
    }

    async fn run(&self, args: &Value) -> Result<String, String> {
        let s = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim);
        let action = s("action").ok_or("missing 'action'")?;
        let (tool, call_args) = match action {
            "open" => {
                let url = s("url").filter(|u| !u.is_empty()).ok_or("open needs 'url'")?;
                ("browser_navigate", json!({ "url": normalize_url(url) }))
            }
            "snapshot" => ("browser_snapshot", json!({})),
            "click" | "type" | "select" => {
                let r = s("ref")
                    .filter(|r| !r.is_empty())
                    .ok_or_else(|| format!("{action} needs 'ref' (like e12) from the page outline"))?
                    .trim_start_matches("ref=")
                    .to_string();
                let rp = self.ref_param().await?;
                let element = s("element")
                    .filter(|e| !e.is_empty())
                    .map(String::from)
                    .or_else(|| self.label_of(&r))
                    .unwrap_or_else(|| format!("element {r}"));
                let mut a = json!({ "element": element });
                a[rp.as_str()] = json!(r);
                match action {
                    "click" => ("browser_click", a),
                    "type" => {
                        a["text"] = json!(s("text").ok_or("type needs 'text'")?);
                        if args.get("submit").and_then(Value::as_bool).unwrap_or(false) {
                            a["submit"] = json!(true);
                        }
                        ("browser_type", a)
                    }
                    _ => {
                        let values: Vec<String> = match args.get("values") {
                            Some(Value::Array(v)) => v
                                .iter()
                                .filter_map(|x| x.as_str().map(String::from))
                                .collect(),
                            Some(Value::String(v)) => vec![v.clone()],
                            _ => s("text").map(|t| vec![t.to_string()]).unwrap_or_default(),
                        };
                        if values.is_empty() {
                            return Err("select needs 'values' (the option text to choose)".into());
                        }
                        a["values"] = json!(values);
                        ("browser_select_option", a)
                    }
                }
            }
            "press" => {
                let key = s("key").or(s("text")).ok_or("press needs 'key', e.g. Enter or PageDown")?;
                ("browser_press_key", json!({ "key": key }))
            }
            "back" => ("browser_navigate_back", json!({})),
            "wait" => match s("text").filter(|t| !t.is_empty()) {
                Some(t) => ("browser_wait_for", json!({ "text": t })),
                None => {
                    let secs = args
                        .get("seconds")
                        .and_then(Value::as_f64)
                        .unwrap_or(2.0)
                        .clamp(0.5, 30.0);
                    ("browser_wait_for", json!({ "time": secs }))
                }
            },
            "tabs" => {
                let mut a = json!({ "action": s("text").filter(|t| !t.is_empty()).unwrap_or("list") });
                if let Some(i) = args.get("index").and_then(Value::as_u64) {
                    a["index"] = json!(i);
                }
                ("browser_tabs", a)
            }
            "dialog" => {
                let accept = args.get("accept").and_then(Value::as_bool).unwrap_or(false);
                let mut a = json!({ "accept": accept });
                if let Some(t) = s("text") {
                    a["promptText"] = json!(t);
                }
                ("browser_handle_dialog", a)
            }
            "find" => {
                let needle = s("text").filter(|t| !t.is_empty()).ok_or("find needs 'text'")?;
                return Ok(self.find(needle));
            }
            other => {
                return Err(format!(
                    "unknown action {other:?}. Use open, snapshot, click, type, select, press, back, wait, find, tabs or dialog."
                ))
            }
        };
        let raw = self
            .hub
            .call_limited(SERVER, tool, call_args, MAX_PAGE_CHARS)
            .await?;
        Ok(self.absorb(&raw))
    }
}

impl ToolProvider for BrowserTool {
    fn specs(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "browser".into(),
            description: "Control a real web browser: open sites, read pages, click, type, choose options, submit forms. \
                Every action returns the page as an outline in which interactive elements carry refs like [ref=e12]; \
                pass that ref to click/type/select. Actions: open {url}; snapshot (read the page again); click {ref}; \
                type {ref, text, submit?}; select {ref, values}; press {key: Enter, Tab, PageDown, Escape...}; back; \
                wait {seconds | text}; find {text}: search the WHOLE current page for text and show matching lines with \
                their refs (free; use it when the page view was cut off); tabs {text: list|new|close|select, index}; \
                dialog {accept, text?}. Clicking things that submit, send, pay or delete asks the user first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["open", "snapshot", "click", "type", "select", "press", "back", "wait", "find", "tabs", "dialog"]},
                    "url": {"type": "string"},
                    "ref": {"type": "string", "description": "Element ref from the page outline, e.g. e12"},
                    "text": {"type": "string"},
                    "values": {"type": "array", "items": {"type": "string"}},
                    "submit": {"type": "boolean"},
                    "key": {"type": "string"},
                    "seconds": {"type": "number"},
                    "index": {"type": "integer"},
                    "accept": {"type": "boolean"},
                    "element": {"type": "string", "description": "Optional: what the element is, in words"}
                },
                "required": ["action"]
            }),
        }]
    }

    fn handles(&self, name: &str) -> bool {
        name == "browser"
    }

    fn risk(&self, _name: &str, args: &Value) -> Risk {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
        let on = match self.url() {
            u if u.is_empty() => String::new(),
            u => format!(" on {u}"),
        };
        match s("action") {
            "click" | "type" => {
                let label = self.label_of(s("ref")).unwrap_or_default();
                let desc = if label.is_empty() {
                    s("element").to_string()
                } else {
                    label
                };
                let desc = if desc.is_empty() {
                    format!("element {}", s("ref"))
                } else {
                    desc
                };
                if s("action") == "type" {
                    if args.get("submit").and_then(Value::as_bool).unwrap_or(false) {
                        return Risk::Ask(format!(
                            "type into {desc} and press Enter{on} (this may submit a form)"
                        ));
                    }
                    return Risk::Safe;
                }
                if looks_final(&desc) {
                    Risk::Ask(format!("click {desc}{on}"))
                } else {
                    Risk::Safe
                }
            }
            "press"
                if s("key").eq_ignore_ascii_case("enter")
                    || s("text").eq_ignore_ascii_case("enter") =>
            {
                Risk::Ask(format!("press Enter{on} (this may submit a form)"))
            }
            "dialog" if args.get("accept").and_then(Value::as_bool).unwrap_or(false) => {
                Risk::Ask(format!("accept the page's pop-up dialog{on}"))
            }
            _ => Risk::Safe,
        }
    }

    /// Rules match "action target", e.g. `browser(open https://*.example.com*)`.
    fn subject(&self, _name: &str, args: &Value) -> String {
        let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim();
        match s("action") {
            "open" => format!("open {}", normalize_url(s("url"))),
            a @ ("click" | "type" | "select") => format!(
                "{a} {}",
                self.label_of(s("ref"))
                    .unwrap_or_else(|| s("ref").to_string())
            ),
            a => a.to_string(),
        }
    }

    fn execute<'a>(&'a self, _name: &'a str, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move { self.run(args).await })
    }
}

/// "example.com" -> "https://example.com"; leaves full URLs and about:/file: alone.
pub fn normalize_url(url: &str) -> String {
    let u = url.trim();
    if u.contains("://") || u.starts_with("about:") || u.starts_with("data:") || u.is_empty() {
        u.to_string()
    } else {
        format!("https://{u}")
    }
}

/// Does a button or link label sound like a final action (submit, pay, delete...)?
pub fn looks_final(label: &str) -> bool {
    let lower = label.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().any(|w| FINAL_WORDS.contains(w))
        || words
            .windows(2)
            .any(|p| FINAL_PAIRS.contains(&(p[0], p[1])))
}

/// Remove generated code and empty filler lines from a Playwright MCP response.
pub fn compact_response(raw: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    let mut fences = 0;
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with("### Ran Playwright code") || t.starts_with("- Ran Playwright code") {
            skipping = true;
            fences = 0;
            continue;
        }
        if skipping {
            if t.starts_with("```") {
                fences += 1;
                if fences == 2 {
                    skipping = false;
                }
                continue;
            }
            if t.starts_with("### ") {
                skipping = false;
            } else {
                continue;
            }
        }
        if is_filler(t) {
            continue;
        }
        out.push(line.replace(" [cursor=pointer]", ""));
    }
    let text = out.join("\n");
    text.trim().to_string()
}

/// `- generic [ref=e4]:` and similar lines with a role but no name or text.
fn is_filler(t: &str) -> bool {
    let Some(rest) = t.strip_prefix("- ") else {
        return false;
    };
    let role_end = rest.find([' ', ':']).unwrap_or(rest.len());
    let (role, tail) = rest.split_at(role_end);
    if !FILLER_ROLES.contains(&role) {
        return false;
    }
    // Drop [ref=..], [active] and other attributes; anything left is real content.
    let mut left = String::new();
    let mut depth = 0;
    for c in tail.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => left.push(c),
            _ => {}
        }
    }
    matches!(left.trim(), "" | ":")
}

fn page_url(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        l.trim()
            .trim_start_matches("- ")
            .strip_prefix("Page URL:")
            .map(|u| u.trim().to_string())
    })
}

/// The outline line for `[ref=r]`, without the ref and other attributes,
/// e.g. `button "Submit quiz"`.
pub fn ref_label(page: &str, r: &str) -> Option<String> {
    let r = r.trim().trim_start_matches("ref=");
    if r.is_empty() {
        return None;
    }
    let marker = format!("[ref={r}]");
    let line = page.lines().find(|l| l.contains(&marker))?;
    let mut label = String::new();
    let mut depth = 0;
    for c in line.trim().trim_start_matches("- ").chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => label.push(c),
            _ => {}
        }
    }
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    let label = label.trim_end_matches(':').trim().to_string();
    (!label.is_empty()).then_some(label)
}

/// The first `max` characters, cut at a line end, with a note on how to see more.
pub fn view(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut = String::new();
    for line in text.lines() {
        if cut.chars().count() + line.chars().count() + 1 > max {
            break;
        }
        cut.push_str(line);
        cut.push('\n');
    }
    format!(
        "{cut}[… page view cut at {max} of {} characters. Use action=find with a word you expect \
         (for example \"Question 3\" or \"Next\") to see the rest with refs.]",
        text.chars().count()
    )
}

/// Lines of the page containing `needle` (case-insensitive), each with the
/// lines below it (answer options usually follow their question).
pub fn find_in_page(page: &str, url: &str, needle: &str) -> String {
    let lines: Vec<&str> = page.lines().collect();
    let n = needle.to_lowercase();
    let mut keep = vec![false; lines.len()];
    let mut hits = 0;
    for (i, l) in lines.iter().enumerate() {
        if l.to_lowercase().contains(&n) {
            hits += 1;
            for k in keep.iter_mut().skip(i).take(FIND_CONTEXT_LINES + 1) {
                *k = true;
            }
        }
    }
    if hits == 0 {
        return format!("\"{needle}\" was not found on the current page ({url}). Try another word, or snapshot if the page changed.");
    }
    let mut out = format!("{hits} match(es) for \"{needle}\" on {url}:\n");
    let mut shown = 0;
    let mut last = None;
    for (i, l) in lines.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if shown >= FIND_MAX_LINES {
            out.push_str("[… more matches; search for something more specific]\n");
            break;
        }
        if last.is_some_and(|p: usize| p + 1 != i) {
            out.push_str("  ⋯\n");
        }
        out.push_str(l);
        out.push('\n');
        shown += 1;
        last = Some(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = r#"### Ran Playwright code
```js
await page.getByRole('button', { name: 'Next' }).click();
```

### Page state
- Page URL: http://127.0.0.1:8765/index.html
- Page Title: Practice Quiz
- Page Snapshot:
```yaml
- generic [active] [ref=e1]:
  - heading "Practice Quiz" [level=1] [ref=e3]
  - generic [ref=e4]:
    - paragraph [ref=e7]: "Question 1: What is 2 + 3?"
    - generic [ref=e8]:
      - radio "4" [ref=e9]
      - text: "4"
      - radio "5" [ref=e10]
    - combobox [ref=e12]:
      - option "Berlin" [selected]
      - option "Paris"
    - textbox "Your name" [ref=e14]
    - button "Submit quiz" [ref=e20] [cursor=pointer]
    - link "Next page" [ref=e21] [cursor=pointer]:
      - /url: /page2
```"#;

    #[test]
    fn compaction_drops_code_and_filler_but_keeps_content() {
        let c = compact_response(RESPONSE);
        assert!(!c.contains("getByRole"), "generated code removed:\n{c}");
        assert!(!c.contains("generic"), "filler removed:\n{c}");
        assert!(!c.contains("cursor=pointer"));
        assert!(c.contains("- paragraph [ref=e7]: \"Question 1: What is 2 + 3?\""));
        assert!(c.contains("radio \"5\" [ref=e10]"));
        assert!(c.contains("/url: /page2"));
        assert_eq!(
            page_url(&c).as_deref(),
            Some("http://127.0.0.1:8765/index.html")
        );
        // A named group or one with text is content, not filler.
        assert!(!is_filler("- group \"Shipping address\" [ref=e5]:"));
        assert!(!is_filler("- generic [ref=e6]: Hello"));
        assert!(is_filler("- listitem [ref=e7]:"));
    }

    #[test]
    fn labels_come_from_the_outline() {
        let c = compact_response(RESPONSE);
        assert_eq!(
            ref_label(&c, "e20").as_deref(),
            Some("button \"Submit quiz\"")
        );
        assert_eq!(ref_label(&c, "ref=e9").as_deref(), Some("radio \"4\""));
        assert_eq!(ref_label(&c, "e99"), None);
    }

    #[test]
    fn final_actions_are_recognised() {
        for yes in [
            "button \"Submit quiz\"",
            "Pay now",
            "Place order",
            "Sign up",
            "button \"Delete account\"",
            "Check out",
        ] {
            assert!(looks_final(yes), "{yes}");
        }
        for no in [
            "button \"Next\"",
            "radio \"4\"",
            "link \"Facebook\"",
            "Previous question",
            "Show answer",
        ] {
            assert!(!looks_final(no), "{no}");
        }
    }

    #[test]
    fn risk_uses_the_real_label_not_the_models_description() {
        let tool = BrowserTool::with_server(ServerConfig {
            name: "x".into(),
            description: None,
            transport: Transport::Stdio {
                command: "none".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            timeout: Duration::from_secs(1),
        });
        let _ = tool.absorb(RESPONSE);
        // The model calls the submit button "the button"; the outline says "Submit quiz".
        let submit = json!({"action":"click","ref":"e20","element":"the button"});
        assert!(
            matches!(tool.risk("browser", &submit), Risk::Ask(q) if q.contains("Submit quiz") && q.contains("127.0.0.1"))
        );
        let answer = json!({"action":"click","ref":"e10"});
        assert_eq!(tool.risk("browser", &answer), Risk::Safe);
        assert_eq!(
            tool.risk(
                "browser",
                &json!({"action":"type","ref":"e14","text":"Ann"})
            ),
            Risk::Safe
        );
        assert!(matches!(
            tool.risk(
                "browser",
                &json!({"action":"type","ref":"e14","text":"Ann","submit":true})
            ),
            Risk::Ask(_)
        ));
        assert!(matches!(
            tool.risk("browser", &json!({"action":"press","key":"Enter"})),
            Risk::Ask(_)
        ));
        assert_eq!(
            tool.risk("browser", &json!({"action":"open","url":"example.com"})),
            Risk::Safe
        );
        assert_eq!(
            tool.subject("browser", &submit),
            "click button \"Submit quiz\""
        );
        assert_eq!(
            tool.subject("browser", &json!({"action":"open","url":"example.com"})),
            "open https://example.com"
        );
    }

    #[test]
    fn find_shows_matches_with_following_lines() {
        let c = compact_response(RESPONSE);
        let f = find_in_page(&c, "u", "question 1");
        assert!(f.starts_with("1 match(es)"));
        assert!(f.contains("radio \"4\" [ref=e9]"), "{f}");
        assert!(find_in_page(&c, "u", "zebra").contains("not found"));
    }

    #[test]
    fn long_pages_are_cut_with_a_hint() {
        let page = (0..2000)
            .map(|i| format!("- text: line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let v = view(&page, 1000);
        assert!(v.len() < 1300);
        assert!(v.contains("Use action=find"));
        assert_eq!(view("short", 1000), "short");
    }

    #[test]
    fn server_args_per_mode() {
        let mut s = BrowserSettings {
            mode: BrowserMode::Own,
            browser: Some("msedge".into()),
            headless: false,
            profile_dir: Some(PathBuf::from("/p")),
            command: "npx".into(),
            package: "@playwright/mcp@latest".into(),
            timeout: Duration::from_secs(60),
        };
        assert_eq!(
            s.server_args(),
            [
                "-y",
                "@playwright/mcp@latest",
                "--user-data-dir=/p",
                "--browser=msedge"
            ]
        );
        s.mode = BrowserMode::Extension;
        assert_eq!(
            s.server_args(),
            [
                "-y",
                "@playwright/mcp@latest",
                "--extension",
                "--browser=msedge"
            ]
        );
        assert_eq!(normalize_url("example.com/a"), "https://example.com/a");
        assert_eq!(normalize_url("http://x"), "http://x");
    }
}
