//! Integration tests for Ollama client: format_bytes, check_status, list_models,
//! pull_model_stream, delete_model, speed_test.

use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use oblivion_core::ollama::{format_bytes, OllamaClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Spins up a lightweight mock HTTP server for Ollama tests.
async fn mock_ollama_server(
    responses: Vec<(u16, &'static str, String)>,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_clone = requests.clone();

    tokio::spawn(async move {
        for (status, content_type, body) in responses {
            let (mut sock, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => return,
            };

            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = sock.read(&mut tmp).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }

            let req_str = String::from_utf8_lossy(&buf).to_string();
            requests_clone.lock().unwrap().push(req_str);

            let status_line = match status {
                200 => "HTTP/1.1 200 OK\r\n",
                404 => "HTTP/1.1 404 Not Found\r\n",
                500 => "HTTP/1.1 500 Internal Server Error\r\n",
                _ => "HTTP/1.1 200 OK\r\n",
            };

            let response = format!(
                "{status_line}Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(response.as_bytes()).await;
            let _ = sock.flush().await;
        }
    });

    (format!("http://{addr}"), requests)
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1024 * 500), "500.0 KB");
    assert_eq!(format_bytes(1024 * 1024 * 10), "10.0 MB");
    assert_eq!(format_bytes(4_683_074_360), "4.36 GB");
}

#[tokio::test]
async fn test_check_status_offline() {
    // Unbound local port
    let client = OllamaClient::new("http://127.0.0.1:59999");
    let status = client.check_status().await;
    assert!(!status.running);
    assert!(status.version.is_none());
    assert!(status.error.is_some());
}

#[tokio::test]
async fn test_check_status_online() {
    let (url, _reqs) = mock_ollama_server(vec![(
        200,
        "application/json",
        r#"{"version":"0.5.4"}"#.to_string(),
    )])
    .await;

    let client = OllamaClient::new(&url);
    let status = client.check_status().await;
    assert!(status.running);
    assert_eq!(status.version.as_deref(), Some("0.5.4"));
    assert!(status.error.is_none());
}

#[tokio::test]
async fn test_list_models() {
    let mock_json = r#"{
        "models": [
            {
                "name": "qwen2.5-coder:7b",
                "modified_at": "2026-11-18T10:00:00Z",
                "size": 4683074360,
                "digest": "sha256:00e13365561b",
                "details": {
                    "format": "gguf",
                    "family": "qwen2",
                    "parameter_size": "7.6B",
                    "quantization_level": "Q4_K_M"
                }
            },
            {
                "name": "llama3.2:3b",
                "modified_at": "2026-11-18T11:00:00Z",
                "size": 2019273840,
                "digest": "sha256:11a22b33c44d",
                "details": {
                    "format": "gguf",
                    "family": "llama",
                    "parameter_size": "3.2B",
                    "quantization_level": "Q4_K_M"
                }
            }
        ]
    }"#;

    let (url, _reqs) =
        mock_ollama_server(vec![(200, "application/json", mock_json.to_string())]).await;

    let client = OllamaClient::new(&url);
    let models = client.list_models().await.unwrap();
    assert_eq!(models.len(), 2);

    assert_eq!(models[0].name, "qwen2.5-coder:7b");
    assert_eq!(models[0].size, 4683074360);
    assert_eq!(models[0].size_formatted, "4.36 GB");
    assert_eq!(
        models[0]
            .details
            .as_ref()
            .unwrap()
            .parameter_size
            .as_deref(),
        Some("7.6B")
    );
    assert_eq!(
        models[0]
            .details
            .as_ref()
            .unwrap()
            .quantization_level
            .as_deref(),
        Some("Q4_K_M")
    );

    assert_eq!(models[1].name, "llama3.2:3b");
    assert_eq!(models[1].size_formatted, "1.88 GB");
}

#[tokio::test]
async fn test_pull_model_stream() {
    let mock_stream = "{\"status\":\"pulling manifest\"}\n{\"status\":\"downloading 00e13365561b\",\"digest\":\"sha256:00e133\",\"total\":1000,\"completed\":500}\n{\"status\":\"verifying sha256 digest\"}\n{\"status\":\"success\"}\n";

    let (url, reqs) =
        mock_ollama_server(vec![(200, "application/x-ndjson", mock_stream.to_string())]).await;

    let client = OllamaClient::new(&url);
    let mut stream = client.pull_model_stream("qwen2.5-coder:7b").await.unwrap();

    let mut events = Vec::new();
    while let Some(res) = stream.next().await {
        let ev = res.unwrap();
        events.push(ev);
    }

    assert_eq!(events.len(), 4);
    assert_eq!(events[0].status, "pulling manifest");
    assert_eq!(events[1].status, "downloading 00e13365561b");
    assert_eq!(events[1].total, Some(1000));
    assert_eq!(events[1].completed, Some(500));
    assert_eq!(events[1].percent, Some(50.0));
    assert_eq!(events[3].status, "success");

    let recorded = reqs.lock().unwrap();
    assert!(recorded[0].contains("POST /api/pull"));
    assert!(recorded[0].contains("qwen2.5-coder:7b"));
}

#[tokio::test]
async fn test_delete_model() {
    let (url, reqs) = mock_ollama_server(vec![(
        200,
        "application/json",
        r#"{"status":"success"}"#.into(),
    )])
    .await;

    let client = OllamaClient::new(&url);
    let res = client.delete_model("qwen2.5-coder:7b").await;
    assert!(res.is_ok());

    let recorded = reqs.lock().unwrap();
    assert!(recorded[0].contains("DELETE /api/delete"));
}

#[tokio::test]
async fn test_speed_test() {
    let mock_gen = r#"{
        "model": "qwen2.5-coder:7b",
        "created_at": "2026-11-19T10:00:00Z",
        "response": "Gravity is the fundamental force of attraction between objects with mass.",
        "done": true,
        "total_duration": 2000000000,
        "load_duration": 500000000,
        "prompt_eval_count": 20,
        "prompt_eval_duration": 200000000,
        "eval_count": 50,
        "eval_duration": 1000000000
    }"#;

    let (url, _reqs) =
        mock_ollama_server(vec![(200, "application/json", mock_gen.to_string())]).await;

    let client = OllamaClient::new(&url);
    let result = client
        .speed_test("qwen2.5-coder:7b", Some("What is gravity?"))
        .await
        .unwrap();

    assert_eq!(result.model, "qwen2.5-coder:7b");
    assert_eq!(result.eval_count, 50);
    assert_eq!(result.eval_duration_ms, 1000.0);
    // 50 tokens / 1.0 second = 50.0 tok/s
    assert!((result.tokens_per_second - 50.0).abs() < 0.01);
    // 20 tokens / 0.2 second = 100.0 tok/s
    assert!((result.prompt_eval_tokens_per_second - 100.0).abs() < 0.01);
    assert_eq!(result.load_duration_ms, 500.0);
    assert_eq!(result.total_duration_ms, 2000.0);
    assert!(result
        .sample_text
        .contains("Gravity is the fundamental force"));
}
