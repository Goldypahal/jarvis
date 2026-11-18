//! Embedding generation for vector search (Ollama + sqlite-vec).
//!
//! Provides the `Embedder` trait, an `OllamaEmbedder` for local embedding models
//! (such as `nomic-embed-text` or `all-minilm`), and a `MockEmbedder` for testing.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

pub type EmbedFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + 'a>>;

/// Trait for converting text into normalized dense vector embeddings.
pub trait Embedder: Send + Sync {
    /// Generate an embedding vector for the given text.
    fn embed<'a>(&'a self, text: &'a str) -> EmbedFuture<'a>;

    /// Model identifier or name.
    fn model_name(&self) -> &str;
}

/// Embedder using a local Ollama instance (e.g. `http://localhost:11434`).
#[derive(Debug, Clone)]
pub struct OllamaEmbedder {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct OllamaEmbeddingsRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

#[derive(Deserialize)]
struct OllamaEmbeddingsResponse {
    embedding: Option<Vec<f32>>,
}

#[derive(Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Option<Vec<Vec<f32>>>,
}

impl OllamaEmbedder {
    pub fn new(endpoint: &str, model: &str) -> Self {
        let ep = if endpoint.trim().is_empty() {
            "http://localhost:11434"
        } else {
            endpoint.trim_end_matches('/')
        };
        Self {
            endpoint: ep.to_string(),
            model: model.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
        }
    }

    async fn request_embeddings(&self, text: &str) -> Result<Vec<f32>> {
        // 1. Try Ollama `/api/embeddings` format
        let url_embeddings = format!("{}/api/embeddings", self.endpoint);
        let req_body = OllamaEmbeddingsRequest {
            model: &self.model,
            prompt: text,
        };

        let resp_result = self
            .client
            .post(&url_embeddings)
            .json(&req_body)
            .send()
            .await;

        match resp_result {
            Ok(resp) if resp.status().is_success() => {
                let data: OllamaEmbeddingsResponse = resp
                    .json()
                    .await
                    .context("invalid JSON from Ollama /api/embeddings")?;
                if let Some(vec) = data.embedding {
                    if !vec.is_empty() {
                        return Ok(vec);
                    }
                }
            }
            Ok(resp) if resp.status().as_u16() == 404 => {
                // Endpoint might be newer Ollama `/api/embed`
            }
            Ok(resp) => {
                let status = resp.status();
                let err_text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("Ollama error {status}: {err_text}"));
            }
            Err(e) => {
                return Err(anyhow!(
                    "failed to connect to Ollama at {}: {e}",
                    self.endpoint
                ));
            }
        }

        // 2. Fallback to Ollama `/api/embed` format
        let url_embed = format!("{}/api/embed", self.endpoint);
        let req_embed_body = OllamaEmbedRequest {
            model: &self.model,
            input: text,
        };

        let resp = self
            .client
            .post(&url_embed)
            .json(&req_embed_body)
            .send()
            .await
            .context("failed to query Ollama /api/embed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama /api/embed error {status}: {err_text}"));
        }

        let embed_data: OllamaEmbedResponse = resp
            .json()
            .await
            .context("invalid JSON from Ollama /api/embed")?;

        if let Some(mut vecs) = embed_data.embeddings {
            if let Some(first) = vecs.pop() {
                if !first.is_empty() {
                    return Ok(first);
                }
            }
        }

        Err(anyhow!("no embedding returned from Ollama"))
    }
}

impl Embedder for OllamaEmbedder {
    fn embed<'a>(&'a self, text: &'a str) -> EmbedFuture<'a> {
        Box::pin(async move { self.request_embeddings(text).await })
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Deterministic mock embedder generating normalized vectors of specified dimensions.
#[derive(Debug, Clone)]
pub struct MockEmbedder {
    model: String,
    dimensions: usize,
}

impl MockEmbedder {
    pub fn new(dimensions: usize) -> Self {
        Self {
            model: "mock-embed".to_string(),
            dimensions,
        }
    }

    pub fn compute_vector(&self, text: &str) -> Vec<f32> {
        let mut v = vec![0.0f32; self.dimensions];
        if self.dimensions == 0 {
            return v;
        }
        for (i, b) in text.as_bytes().iter().enumerate() {
            v[i % self.dimensions] += (*b as f32) / 255.0;
        }
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-6 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

impl Embedder for MockEmbedder {
    fn embed<'a>(&'a self, text: &'a str) -> EmbedFuture<'a> {
        let vec = self.compute_vector(text);
        Box::pin(async move { Ok(vec) })
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_embedder_generates_normalized_vectors() {
        let embedder = MockEmbedder::new(4);
        let v1 = embedder.embed("rust tokio").await.unwrap();
        assert_eq!(v1.len(), 4);
        let norm: f32 = v1.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);

        // Different text produces different vectors
        let v2 = embedder.embed("python flask").await.unwrap();
        assert_ne!(v1, v2);

        // Same text produces identical vectors
        let v1_again = embedder.embed("rust tokio").await.unwrap();
        assert_eq!(v1, v1_again);
    }

    #[tokio::test]
    async fn ollama_embedder_handles_offline_gracefully() {
        let embedder = OllamaEmbedder::new("http://127.0.0.1:54321", "nomic-embed-text");
        let res = embedder.embed("testing offline connection").await;
        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(err_msg.contains("failed to connect to Ollama") || err_msg.contains("error"));
    }
}
