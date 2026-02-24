use futures::future::BoxFuture;
use serde::Deserialize;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct WebFetchTool;

#[derive(Deserialize)]
struct WebFetchInput {
    url: String,
    #[serde(default)]
    max_length: Option<usize>,
}

impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T1
    }

    fn timeout_secs(&self) -> u64 {
        60
    }

    fn description(&self) -> &str {
        "Fetch content from a URL. Strips HTML tags and returns plain text, truncated to max_length."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch content from"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum characters to return (default: 30000)"
                }
            },
            "required": ["url"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: WebFetchInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let max_length = params.max_length.unwrap_or(30_000);

            debug!(url = %params.url, "Fetching URL");

            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent("Ryvos/0.1")
                .build()
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "web_fetch".to_string(),
                    message: format!("Failed to create HTTP client: {}", e),
                })?;

            let resp = client
                .get(&params.url)
                .send()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "web_fetch".to_string(),
                    message: format!("Request failed: {}", e),
                })?;

            let status = resp.status();
            if !status.is_success() {
                return Ok(ToolResult::error(format!(
                    "HTTP {} {}",
                    status.as_u16(),
                    status.canonical_reason().unwrap_or("Unknown")
                )));
            }

            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            let body = resp.text().await.map_err(|e| RyvosError::ToolExecution {
                tool: "web_fetch".to_string(),
                message: format!("Failed to read response body: {}", e),
            })?;

            // Strip HTML tags if content is HTML
            let text = if content_type.contains("html") {
                strip_html_tags(&body)
            } else {
                body
            };

            // Truncate
            let output = if text.len() > max_length {
                format!("{}\n\n[truncated at {} chars]", &text[..max_length], max_length)
            } else {
                text
            };

            Ok(ToolResult::success(output))
        })
    }
}

/// Basic HTML tag stripping using regex.
fn strip_html_tags(html: &str) -> String {
    // Remove script and style blocks entirely
    let re_script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let cleaned = re_script.replace_all(html, "");
    let re_style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let cleaned = re_style.replace_all(&cleaned, "");

    // Remove HTML tags
    let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&cleaned, "");

    // Decode common HTML entities
    let text = text
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");

    // Collapse multiple whitespace/newlines
    let re_whitespace = regex::Regex::new(r"\n{3,}").unwrap();
    let text = re_whitespace.replace_all(&text, "\n\n");

    text.trim().to_string()
}
