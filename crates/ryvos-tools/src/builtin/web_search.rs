use futures::future::BoxFuture;
use serde_json::json;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct WebSearchTool {
    api_key: String,
    http: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            http: reqwest::Client::new(),
        }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T3
    }

    fn description(&self) -> &str {
        "Search the web for current information. Returns relevant results with snippets."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default 5)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let query = input["query"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolValidation("'query' must be a string".into()))?;
            let max = input["max_results"].as_u64().unwrap_or(5);

            let resp = self
                .http
                .post("https://api.tavily.com/search")
                .json(&json!({
                    "api_key": self.api_key,
                    "query": query,
                    "max_results": max,
                }))
                .send()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "web_search".into(),
                    message: e.to_string(),
                })?;

            let body: serde_json::Value =
                resp.json().await.map_err(|e| RyvosError::ToolExecution {
                    tool: "web_search".into(),
                    message: e.to_string(),
                })?;

            let results = body["results"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|r| {
                            format!(
                                "**{}**\n{}\nURL: {}",
                                r["title"].as_str().unwrap_or(""),
                                r["content"].as_str().unwrap_or(""),
                                r["url"].as_str().unwrap_or("")
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n---\n\n")
                })
                .unwrap_or_else(|| "No results found.".into());

            Ok(ToolResult::success(results))
        })
    }
}
