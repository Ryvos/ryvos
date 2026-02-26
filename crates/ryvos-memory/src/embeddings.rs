use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

/// Trait for embedding providers (OpenAI-compatible APIs).
pub trait EmbeddingProvider: Send + Sync + 'static {
    /// Embed a batch of texts into vectors.
    fn embed(&self, texts: &[String]) -> BoxFuture<'_, Result<Vec<Vec<f32>>, String>>;
    /// Number of dimensions in the embedding vectors.
    fn dimensions(&self) -> usize;
}

/// HTTP-based embedding provider compatible with OpenAI, Ollama, etc.
pub struct HttpEmbeddingProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
    dims: usize,
}

impl HttpEmbeddingProvider {
    pub fn new(base_url: &str, api_key: Option<&str>, model: &str, dims: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(String::from),
            model: model.to_string(),
            dims,
        }
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingProvider for HttpEmbeddingProvider {
    fn embed(&self, texts: &[String]) -> BoxFuture<'_, Result<Vec<Vec<f32>>, String>> {
        let texts = texts.to_vec();
        Box::pin(async move {
            let url = format!("{}/embeddings", self.base_url);

            let mut req = self.client.post(&url).json(&EmbeddingRequest {
                model: self.model.clone(),
                input: texts,
            });

            if let Some(ref key) = self.api_key {
                req = req.bearer_auth(key);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| format!("Embedding request failed: {}", e))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Embedding API error {}: {}", status, body));
            }

            let body: EmbeddingResponse = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse embedding response: {}", e))?;

            Ok(body.data.into_iter().map(|d| d.embedding).collect())
        })
    }

    fn dimensions(&self) -> usize {
        self.dims
    }
}

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let sim = cosine_similarity(&[], &[]);
        assert_eq!(sim, 0.0);
    }
}
