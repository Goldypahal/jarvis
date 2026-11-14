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
