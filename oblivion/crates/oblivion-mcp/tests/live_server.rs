//! End-to-end over a real stdio MCP connection to a small Python server
//! (tests/fixtures/echo_server.py, standard library only).

use std::collections::BTreeMap;
use std::time::Duration;

use oblivion_mcp::{McpHub, ServerConfig, Transport};
use oblivion_tools::ToolProvider;
use serde_json::json;

fn python() -> String {
    if cfg!(windows) {
        "python".into()
    } else {
        "python3".into()
    }
}

#[tokio::test]
async fn stdio_server_lists_and_calls_tools() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/echo_server.py");
    let hub = McpHub::new(vec![ServerConfig {
        name: "echo".into(),
        description: Some("test server".into()),
        transport: Transport::Stdio {
            command: python(),
            args: vec![script.into()],
            env: BTreeMap::new(),
        },
        timeout: Duration::from_secs(30),
    }]);

    let tools = hub
        .execute("mcp", &json!({"action":"list_tools","server":"echo"}))
        .await
        .unwrap();
    assert!(
        tools.contains("- echo: Echo text back") && tools.contains("{text: string}"),
        "{tools}"
    );
    assert!(tools.contains("{a: number, b: number}"), "{tools}");

    let out = hub
        .execute(
            "mcp",
            &json!({"action":"call","server":"echo","tool":"echo","arguments":{"text":"hello"}}),
        )
        .await
        .unwrap();
    assert_eq!(out, "echo: hello");

    // Arguments sent as a JSON string (some models do this) still work.
    let out = hub
        .execute(
            "mcp",
            &json!({"action":"call","server":"echo","tool":"add","arguments":"{\"a\":2,\"b\":40}"}),
        )
        .await
        .unwrap();
    assert_eq!(out, "42");

    let err = hub
        .execute(
            "mcp",
            &json!({"action":"call","server":"echo","tool":"fail"}),
        )
        .await
        .unwrap_err();
    assert!(
        err.contains("reported an error") && err.contains("always fails"),
        "{err}"
    );

    let err = hub
        .execute("mcp", &json!({"action":"list_tools","server":"nope"}))
        .await
        .unwrap_err();
    assert!(err.contains("Configured: echo"), "{err}");
    hub.shutdown().await;
}

#[tokio::test]
async fn missing_program_is_a_clear_error() {
    let hub = McpHub::new(vec![ServerConfig {
        name: "ghost".into(),
        description: None,
        transport: Transport::Stdio {
            command: "definitely-not-a-real-program-xyz".into(),
            args: vec![],
            env: BTreeMap::new(),
        },
        timeout: Duration::from_secs(10),
    }]);
    let err = hub.list_tools("ghost").await.unwrap_err();
    assert!(
        err.contains("ghost") || err.contains("definitely-not"),
        "{err}"
    );
}

#[tokio::test]
async fn browser_tool_drives_a_playwright_style_server() {
    use oblivion_mcp::BrowserTool;
    use oblivion_tools::Risk;
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fake_playwright.py"
    );
    let browser = BrowserTool::with_server(ServerConfig {
        name: "ignored".into(),
        description: None,
        transport: Transport::Stdio {
            command: python(),
            args: vec![script.into()],
            env: BTreeMap::new(),
        },
        timeout: Duration::from_secs(30),
    });
    browser.check().await.unwrap();

    let page = browser
        .execute("browser", &json!({"action":"open","url":"quiz.example/q1"}))
        .await
        .unwrap();
    assert!(page.contains("Page URL: https://quiz.example/q1"), "{page}");
    assert!(page.contains("radio \"5\" [ref=e10]"));
    assert!(
        !page.contains("page.goto") && !page.contains("generic"),
        "trimmed: {page}"
    );

    // The newer server takes `target`; the label comes from the page outline.
    let out = browser
        .execute("browser", &json!({"action":"click","ref":"e10"}))
        .await
        .unwrap();
    assert!(
        out.contains(r#""args": {"element": "radio \"5\"", "target": "e10"}"#),
        "{out}"
    );

    let out = browser
        .execute(
            "browser",
            &json!({"action":"select","ref":"e9","text":"Paris"}),
        )
        .await
        .unwrap();
    assert!(out.contains(r#""values": ["Paris"]"#), "{out}");

    // Submitting asks first; the question names the real button and page.
    assert!(matches!(
        browser.risk("browser", &json!({"action":"click","ref":"e20"})),
        Risk::Ask(q) if q.contains("Submit quiz") && q.contains("quiz.example")
    ));

    let found = browser
        .execute("browser", &json!({"action":"find","text":"what is 2"}))
        .await
        .unwrap();
    assert!(found.contains("[ref=e10]"), "{found}");

    let err = browser
        .execute("browser", &json!({"action":"click","ref":"e99"}))
        .await
        .unwrap_err();
    assert!(err.contains("not found"), "{err}");
    browser.shutdown().await;
}
