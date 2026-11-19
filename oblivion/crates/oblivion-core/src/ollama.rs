//! Ollama model management client: list, pull with progress, delete, speed test.

use std::pin::Pin;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{stream, Stream, StreamExt};
use serde::{Deserialize, Serialize};

/// Formats bytes into a human-readable size string.
pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Connection status and version of local Ollama server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaStatus {
    pub running: bool,
    pub endpoint: String,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// Details of a local model (architecture, quantization, parameter size).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OllamaModelDetails {
    #[serde(default)]
    pub parent_model: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub families: Option<Vec<String>>,
    #[serde(default)]
    pub parameter_size: Option<String>,
    #[serde(default)]
    pub quantization_level: Option<String>,
}

/// A local model installed in Ollama.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub modified_at: String,
    pub size: u64,
    pub size_formatted: String,
    #[serde(default)]
    pub digest: String,
    pub details: Option<OllamaModelDetails>,
}

/// Raw item from Ollama `GET /api/tags`
#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaRawModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaRawModel {
    name: String,
    #[serde(default)]
    modified_at: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: String,
    #[serde(default)]
    details: Option<OllamaModelDetails>,
}

/// Progress event emitted while pulling a model.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OllamaPullProgress {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of an inference speed test / benchmark on a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaSpeedTestResult {
    pub model: String,
    pub eval_count: u64,
    pub eval_duration_ms: f64,
    pub tokens_per_second: f64,
    pub prompt_eval_count: u64,
    pub prompt_eval_duration_ms: f64,
    pub prompt_eval_tokens_per_second: f64,
    pub load_duration_ms: f64,
    pub total_duration_ms: f64,
    pub sample_text: String,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    #[serde(default)]
    response: String,
    #[serde(default)]
    total_duration: u64,
    #[serde(default)]
    load_duration: u64,
    #[serde(default)]
    prompt_eval_count: u64,
    #[serde(default)]
    prompt_eval_duration: u64,
    #[serde(default)]
    eval_count: u64,
    #[serde(default)]
    eval_duration: u64,
}

/// Client for interacting with local Ollama daemon.
#[derive(Debug, Clone)]
pub struct OllamaClient {
    endpoint: String,
    client: reqwest::Client,
}

impl OllamaClient {
    pub fn new(endpoint: &str) -> Self {
        let ep = if endpoint.trim().is_empty() {
            "http://localhost:11434"
        } else {
            endpoint.trim_end_matches('/')
        };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();
        Self {
            endpoint: ep.to_string(),
            client,
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Checks if Ollama is running and queries its version.
    /// Fast 1.5s timeout so healthchecks never block the UI.
    pub async fn check_status(&self) -> OllamaStatus {
        let url = format!("{}/api/version", self.endpoint);
        let check_client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .unwrap_or_else(|_| self.client.clone());

        match check_client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                #[derive(Deserialize)]
                struct VerResp {
                    version: String,
                }
                let ver = resp.json::<VerResp>().await.map(|v| v.version).ok();
                OllamaStatus {
                    running: true,
                    endpoint: self.endpoint.clone(),
                    version: ver,
                    error: None,
                }
            }
            Ok(resp) => OllamaStatus {
                running: false,
                endpoint: self.endpoint.clone(),
                version: None,
                error: Some(format!("HTTP {}", resp.status())),
            },
            Err(e) => OllamaStatus {
                running: false,
                endpoint: self.endpoint.clone(),
                version: None,
                error: Some(format!("Connection refused: {e}")),
            },
        }
    }

    /// Lists models currently installed locally in Ollama.
    pub async fn list_models(&self) -> Result<Vec<OllamaModel>> {
        let url = format!("{}/api/tags", self.endpoint);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to reach Ollama tags API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama /api/tags error {status}: {err_text}"));
        }

        let tags: OllamaTagsResponse = resp
            .json()
            .await
            .context("invalid JSON from Ollama /api/tags")?;

        let models = tags
            .models
            .into_iter()
            .map(|m| OllamaModel {
                name: m.name,
                modified_at: m.modified_at,
                size_formatted: format_bytes(m.size),
                size: m.size,
                digest: m.digest,
                details: m.details,
            })
            .collect();

        Ok(models)
    }

    /// Pulls a model from the Ollama library, streaming progress events.
    pub async fn pull_model_stream(
        &self,
        model: &str,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<OllamaPullProgress>> + Send>>> {
        let url = format!("{}/api/pull", self.endpoint);
        #[derive(Serialize)]
        struct PullReq<'a> {
            name: &'a str,
            stream: bool,
        }
        let body = PullReq {
            name: model,
            stream: true,
        };

        // Long timeout for model downloads (e.g. 1 hour)
        let download_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3600))
            .build()
            .unwrap_or_else(|_| self.client.clone());

        let resp = download_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("failed to start model pull")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama pull error {status}: {err_text}"));
        }

        let byte_stream = resp.bytes_stream();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<OllamaPullProgress>>(100);

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut byte_stream = Box::pin(byte_stream);

            while let Some(chunk_res) = byte_stream.next().await {
                match chunk_res {
                    Ok(chunk) => {
                        let text = match std::str::from_utf8(&chunk) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        buffer.push_str(text);

                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].trim().to_string();
                            buffer = buffer[newline_pos + 1..].to_string();

                            if line.is_empty() {
                                continue;
                            }

                            #[derive(Deserialize)]
                            struct RawProgress {
                                #[serde(default)]
                                status: String,
                                #[serde(default)]
                                digest: Option<String>,
                                #[serde(default)]
                                total: Option<u64>,
                                #[serde(default)]
                                completed: Option<u64>,
                                #[serde(default)]
                                error: Option<String>,
                            }

                            if let Ok(p) = serde_json::from_str::<RawProgress>(&line) {
                                if let Some(err) = p.error {
                                    let _ = tx.send(Err(anyhow!(err))).await;
                                    return;
                                }

                                let percent = match (p.completed, p.total) {
                                    (Some(c), Some(t)) if t > 0 => {
                                        Some(((c as f64) / (t as f64) * 100.0).clamp(0.0, 100.0))
                                    }
                                    _ => None,
                                };

                                let item = OllamaPullProgress {
                                    status: p.status,
                                    digest: p.digest,
                                    total: p.total,
                                    completed: p.completed,
                                    percent,
                                    error: None,
                                };

                                if tx.send(Ok(item)).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("Stream error: {e}"))).await;
                        return;
                    }
                }
            }
        });

        let s = stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });

        Ok(Box::pin(s))
    }

    /// Deletes a local model from Ollama.
    pub async fn delete_model(&self, model: &str) -> Result<()> {
        let url = format!("{}/api/delete", self.endpoint);
        #[derive(Serialize)]
        struct DeleteReq<'a> {
            name: &'a str,
        }
        let body = DeleteReq { name: model };

        let resp = self
            .client
            .delete(&url)
            .json(&body)
            .send()
            .await
            .context("failed to reach Ollama delete API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama delete error {status}: {err_text}"));
        }

        Ok(())
    }

    /// Performs an inference speed test on an installed model.
    pub async fn speed_test(
        &self,
        model: &str,
        custom_prompt: Option<&str>,
    ) -> Result<OllamaSpeedTestResult> {
        let url = format!("{}/api/generate", self.endpoint);
        let prompt = custom_prompt.unwrap_or("Explain what gravity is in 40 words or less.");

        #[derive(Serialize)]
        struct GenerateReq<'a> {
            model: &'a str,
            prompt: &'a str,
            stream: bool,
            options: serde_json::Value,
        }

        let body = GenerateReq {
            model,
            prompt,
            stream: false,
            options: serde_json::json!({
                "num_predict": 60,
                "temperature": 0.2
            }),
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .context("failed to reach Ollama generate API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama generate error {status}: {err_text}"));
        }

        let gen: OllamaGenerateResponse = resp
            .json()
            .await
            .context("invalid JSON from Ollama /api/generate")?;

        let eval_duration_ms = gen.eval_duration as f64 / 1_000_000.0;
        let tokens_per_second = if gen.eval_duration > 0 {
            (gen.eval_count as f64) / (gen.eval_duration as f64 / 1_000_000_000.0)
        } else {
            0.0
        };

        let prompt_eval_duration_ms = gen.prompt_eval_duration as f64 / 1_000_000.0;
        let prompt_eval_tokens_per_second = if gen.prompt_eval_duration > 0 {
            (gen.prompt_eval_count as f64) / (gen.prompt_eval_duration as f64 / 1_000_000_000.0)
        } else {
            0.0
        };

        let load_duration_ms = gen.load_duration as f64 / 1_000_000.0;
        let total_duration_ms = gen.total_duration as f64 / 1_000_000.0;

        Ok(OllamaSpeedTestResult {
            model: model.to_string(),
            eval_count: gen.eval_count,
            eval_duration_ms,
            tokens_per_second,
            prompt_eval_count: gen.prompt_eval_count,
            prompt_eval_duration_ms,
            prompt_eval_tokens_per_second,
            load_duration_ms,
            total_duration_ms,
            sample_text: gen.response.trim().to_string(),
        })
    }
}
