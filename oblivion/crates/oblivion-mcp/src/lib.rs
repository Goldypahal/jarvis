//! MCP connectors (SRS FR-40/41).
//!
//! Servers are reached over stdio (a local program) or streamable HTTP (a
//! remote URL). They connect lazily on first use, and the model sees ONE tool,
//! `mcp`, instead of every server's tool schemas. A server with 30 tools would
//! otherwise add thousands of tokens to every request, which free tiers with
//! small request limits cannot afford. The model first lists a server's tools,
//! then calls the one it needs.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use oblivion_providers::ToolSpec;
use oblivion_tools::{Risk, ToolFuture, ToolProvider};
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;

mod browser;
pub use browser::{is_insufficient_page_text, BrowserMode, BrowserSettings, BrowserTool};

#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Http {
        url: String,
        token: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerConfig {
    pub name: String,
    pub description: Option<String>,
    pub transport: Transport,
    pub timeout: Duration,
}

type Client = RunningService<RoleClient, ()>;

struct Slot {
    config: ServerConfig,
    client: Mutex<Option<Client>>,
}

pub struct McpHub {
    servers: Vec<Arc<Slot>>,
}

const MAX_SCHEMA_CHARS: usize = 600;
const MAX_RESULT_CHARS: usize = 20_000;

impl McpHub {
    pub fn new(configs: Vec<ServerConfig>) -> Self {
        Self {
            servers: configs
                .into_iter()
                .map(|config| {
                    Arc::new(Slot {
                        config,
                        client: Mutex::new(None),
                    })
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub fn names(&self) -> Vec<String> {
        self.servers.iter().map(|s| s.config.name.clone()).collect()
    }

    /// The system-prompt section listing servers (no tool schemas).
    pub fn prompt_listing(&self) -> String {
        if self.servers.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "MCP connectors\nUse the `mcp` tool to reach these servers: first {\"action\":\"list_tools\",\"server\":...} \
             to see a server's tools and their arguments, then {\"action\":\"call\",\"server\":...,\"tool\":...,\"arguments\":{...}}.\n",
        );
        for s in &self.servers {
            let desc = s
                .config
                .description
                .as_deref()
                .unwrap_or("(no description)");
            out.push_str(&format!("- {}: {desc}\n", s.config.name));
        }
        out
    }

    fn slot(&self, name: &str) -> Result<&Arc<Slot>, String> {
        self.servers
            .iter()
            .find(|s| s.config.name == name)
            .ok_or_else(|| {
                format!(
                    "no MCP server named '{name}'. Configured: {}",
                    self.names().join(", ")
                )
            })
    }

    async fn connect(config: &ServerConfig) -> Result<Client, String> {
        let fut = async {
            match &config.transport {
                Transport::Stdio { command, args, env } => {
                    // On Windows, launchers like npx/uvx are .cmd scripts; going through
                    // cmd.exe finds them the same way a terminal does.
                    let mut cmd = if cfg!(windows) {
                        let mut c = tokio::process::Command::new("cmd");
                        c.arg("/C").arg(command);
                        c
                    } else {
                        tokio::process::Command::new(command)
                    };
                    cmd.args(args).envs(env).kill_on_drop(true);
                    let (transport, _stderr) = TokioChildProcess::builder(cmd)
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|e| format!("could not start '{command}': {e}"))?;
                    ().serve(transport)
                        .await
                        .map_err(|e| format!("MCP server '{}' handshake failed: {e}", config.name))
                }
                Transport::Http { url, token } => {
                    let mut cfg = StreamableHttpClientTransportConfig::with_uri(url.as_str());
                    if let Some(t) = token {
                        cfg = cfg.auth_header(t.clone());
                    }
                    let transport = StreamableHttpClientTransport::from_config(cfg);
                    ().serve(transport)
                        .await
                        .map_err(|e| format!("could not connect to {url}: {e}"))
                }
            }
        };
        tokio::time::timeout(config.timeout, fut)
            .await
            .map_err(|_| {
                format!(
                    "timed out after {}s connecting to MCP server '{}'",
                    config.timeout.as_secs(),
                    config.name
                )
            })?
    }

    /// Run `f` with a connected client, connecting on first use and once more
    /// if the previous connection has died.
    async fn with_client<T, F, Fut>(&self, server: &str, f: F) -> Result<T, String>
    where
        F: Fn(rmcp::Peer<RoleClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let slot = self.slot(server)?.clone();
        for attempt in 0..2 {
            let peer = {
                let mut guard = slot.client.lock().await;
                if guard.as_ref().is_none_or(|c| c.is_transport_closed()) {
                    *guard = Some(Self::connect(&slot.config).await?);
                }
                guard.as_ref().expect("connected").peer().clone()
            };
            match tokio::time::timeout(slot.config.timeout, f(peer)).await {
                Ok(Ok(v)) => return Ok(v),
                Ok(Err(e))
                    if attempt == 0
                        && slot
                            .client
                            .lock()
                            .await
                            .as_ref()
                            .is_some_and(|c| c.is_transport_closed()) =>
                {
                    let _ = e; // connection died mid-call: reconnect and retry once
                    continue;
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(format!(
                        "MCP server '{server}' did not answer within {}s",
                        slot.config.timeout.as_secs()
                    ))
                }
            }
        }
        Err(format!("MCP server '{server}' keeps disconnecting"))
    }

    /// Compact tool list: name, description and argument schema per tool.
    pub async fn list_tools(&self, server: &str) -> Result<String, String> {
        let tools = self
            .with_client(server, |peer| async move {
                peer.list_all_tools().await.map_err(|e| e.to_string())
            })
            .await?;
        if tools.is_empty() {
            return Ok(format!("Server '{server}' has no tools."));
        }
        let mut out = format!("Tools on '{server}' ({}):\n", tools.len());
        for t in tools {
            let desc: String = t
                .description
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let desc: String = desc.chars().take(300).collect();
            let schema = compact_schema(&Value::Object((*t.input_schema).clone()));
            out.push_str(&format!("- {}: {desc}\n  arguments: {schema}\n", t.name));
        }
        Ok(out)
    }

    /// Argument names a server's tool accepts (empty if the tool does not exist).
    pub async fn tool_params(&self, server: &str, tool: &str) -> Result<Vec<String>, String> {
        let tools = self
            .with_client(server, |peer| async move {
                peer.list_all_tools().await.map_err(|e| e.to_string())
            })
            .await?;
        Ok(tools
            .iter()
            .find(|t| t.name == tool)
            .and_then(|t| t.input_schema.get("properties"))
            .and_then(Value::as_object)
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default())
    }

    pub async fn call(&self, server: &str, tool: &str, arguments: Value) -> Result<String, String> {
        self.call_limited(server, tool, arguments, MAX_RESULT_CHARS)
            .await
    }

    /// Like [`call`](Self::call), with a custom cap on the result text.
    pub async fn call_limited(
        &self,
        server: &str,
        tool: &str,
        arguments: Value,
        max_chars: usize,
    ) -> Result<String, String> {
        let v = self.call_raw(server, tool, arguments).await?;
        let text = result_text_max(&v, max_chars);
        if v.get("isError").and_then(Value::as_bool).unwrap_or(false) {
            return Err(format!("{server}.{tool} reported an error: {text}"));
        }
        Ok(text)
    }

    /// Call an MCP tool and return the parsed JSON response value from the server.
    pub async fn call_raw(
        &self,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let args = match arguments {
            Value::Object(m) => Some(m),
            Value::Null => None,
            other => return Err(format!("arguments must be a JSON object, got {other}")),
        };
        let tool_name = tool.to_string();
        let result = self
            .with_client(server, |peer| {
                let mut params = CallToolRequestParams::new(tool_name.clone());
                params.arguments = args.clone();
                async move { peer.call_tool(params).await.map_err(|e| e.to_string()) }
            })
            .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }

    /// Close every connection (stops stdio server processes).
    pub async fn shutdown(&self) {
        for s in &self.servers {
            if let Some(c) = s.client.lock().await.take() {
                let _ = c.cancel().await;
            }
        }
    }
}

/// Text from an MCP tool result: text blocks verbatim, other blocks summarized,
/// structured content as JSON when there is no text.
pub fn result_text(v: &Value) -> String {
    result_text_max(v, MAX_RESULT_CHARS)
}

fn result_text_max(v: &Value, max_chars: usize) -> String {
    let mut parts = Vec::new();
    for block in v
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => parts.push(
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
            Some("image") => parts.push(format!(
                "[image: {}]",
                block.get("mimeType").and_then(Value::as_str).unwrap_or("?")
            )),
            Some("resource") | Some("resource_link") => {
                let r = block.get("resource").unwrap_or(block);
                let uri = r.get("uri").and_then(Value::as_str).unwrap_or("?");
                match r.get("text").and_then(Value::as_str) {
                    Some(t) => parts.push(format!("[resource {uri}]\n{t}")),
                    None => parts.push(format!("[resource {uri}]")),
                }
            }
            Some(other) => parts.push(format!("[{other} content]")),
            None => {}
        }
    }
    if parts.is_empty() {
        if let Some(s) = v.get("structuredContent") {
            parts.push(s.to_string());
        }
    }
    let text = parts.join("\n");
    if text.chars().count() > max_chars {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{cut}\n[... result truncated]")
    } else if text.is_empty() {
        "(no output)".into()
    } else {
        text
    }
}

/// Argument schema boiled down to `{name: type (required), ...}`, so listing a
/// server's tools stays cheap in tokens.
pub fn compact_schema(schema: &Value) -> String {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return "{}".into();
    };
    let fields: Vec<String> = props
        .iter()
        .map(|(k, v)| {
            let ty = match v.get("type") {
                Some(Value::String(t)) if t == "array" => {
                    let inner = v
                        .pointer("/items/type")
                        .and_then(Value::as_str)
                        .unwrap_or("any");
                    format!("{inner}[]")
                }
                Some(Value::String(t)) => t.clone(),
                Some(Value::Array(ts)) => ts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|"),
                _ if v.get("enum").is_some() => "enum".into(),
                _ => "any".into(),
            };
            let ty = match v.get("enum").and_then(Value::as_array) {
                Some(vals) => vals
                    .iter()
                    .take(8)
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join("|"),
                None => ty,
            };
            let req = if required.contains(&k.as_str()) {
                ""
            } else {
                "?"
            };
            format!("{k}{req}: {ty}")
        })
        .collect();
    let s = format!("{{{}}}", fields.join(", "));
    if s.len() > MAX_SCHEMA_CHARS {
        format!(
            "{}…}}",
            &s[..s
                .char_indices()
                .take_while(|(i, _)| *i < MAX_SCHEMA_CHARS)
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0)]
        )
    } else {
        s
    }
}

impl ToolProvider for McpHub {
    fn specs(&self) -> Vec<ToolSpec> {
        if self.servers.is_empty() {
            return vec![];
        }
        vec![ToolSpec {
            name: "mcp".into(),
            description: format!(
                "Use a connected MCP server ({}). action=list_tools shows a server's tools and arguments; \
                 action=call runs one tool with JSON arguments.",
                self.names().join(", ")
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list_tools", "call"]},
                    "server": {"type": "string"},
                    "tool": {"type": "string", "description": "Tool name (for action=call)"},
                    "arguments": {"type": "object", "description": "Tool arguments (for action=call)"}
                },
                "required": ["action", "server"]
            }),
        }]
    }

    fn handles(&self, name: &str) -> bool {
        name == "mcp"
    }

    fn risk(&self, _name: &str, args: &Value) -> Risk {
        let get = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string()
        };
        match args.get("action").and_then(Value::as_str) {
            Some("list_tools") => Risk::Safe,
            _ => Risk::Ask(format!(
                "call MCP tool {}.{} with {}",
                get("server"),
                get("tool"),
                args.get("arguments")
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "{}".into())
                    .chars()
                    .take(200)
                    .collect::<String>()
            )),
        }
    }

    /// Rules match "server.tool", e.g. `mcp(github.list_*)` or `mcp(filesystem.*)`.
    fn subject(&self, _name: &str, args: &Value) -> String {
        let get = |k: &str| {
            args.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        match args.get("action").and_then(Value::as_str) {
            Some("list_tools") => format!("{}.list_tools", get("server")),
            _ => format!("{}.{}", get("server"), get("tool")),
        }
    }

    fn execute<'a>(&'a self, _name: &'a str, args: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            let server = args
                .get("server")
                .and_then(Value::as_str)
                .ok_or("missing 'server'")?;
            match args.get("action").and_then(Value::as_str) {
                Some("list_tools") => self.list_tools(server).await,
                Some("call") => {
                    let tool = args
                        .get("tool")
                        .and_then(Value::as_str)
                        .ok_or("missing 'tool' for action=call")?;
                    let arguments = args.get("arguments").cloned().unwrap_or(Value::Null);
                    // Some models send the arguments as a JSON string.
                    let arguments = match arguments {
                        Value::String(s) => serde_json::from_str(&s)
                            .map_err(|e| format!("arguments is not valid JSON: {e}"))?,
                        other => other,
                    };
                    self.call(server, tool, arguments).await
                }
                other => Err(format!("unknown action {other:?}; use list_tools or call")),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_compacted() {
        let s = json!({"type":"object","properties":{
            "path":{"type":"string"},"limit":{"type":"integer"},"tags":{"type":"array","items":{"type":"string"}},
            "mode":{"enum":["a","b"]},"x":{"type":["string","null"]}},"required":["path"]});
        assert_eq!(
            compact_schema(&s),
            r#"{limit?: integer, mode?: "a"|"b", path: string, tags?: string[], x?: string|null}"#
        );
        assert_eq!(compact_schema(&json!({"type":"object"})), "{}");
    }

    #[test]
    fn result_text_handles_block_types() {
        let v = json!({"content":[{"type":"text","text":"hello"},{"type":"image","mimeType":"image/png","data":"..."},
            {"type":"resource","resource":{"uri":"file:///a","text":"body"}}],"isError":false});
        assert_eq!(
            result_text(&v),
            "hello\n[image: image/png]\n[resource file:///a]\nbody"
        );
        assert_eq!(
            result_text(&json!({"content":[],"structuredContent":{"n":1}})),
            r#"{"n":1}"#
        );
        assert_eq!(result_text(&json!({"content":[]})), "(no output)");
    }

    #[test]
    fn risk_and_subject() {
        let hub = McpHub::new(vec![ServerConfig {
            name: "fs".into(),
            description: Some("files".into()),
            transport: Transport::Stdio {
                command: "x".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            timeout: Duration::from_secs(5),
        }]);
        let list = json!({"action":"list_tools","server":"fs"});
        let call =
            json!({"action":"call","server":"fs","tool":"read_file","arguments":{"path":"a"}});
        assert_eq!(hub.risk("mcp", &list), Risk::Safe);
        assert!(matches!(hub.risk("mcp", &call), Risk::Ask(s) if s.contains("fs.read_file")));
        assert_eq!(hub.subject("mcp", &call), "fs.read_file");
        assert!(hub.prompt_listing().contains("- fs: files"));
    }
}
