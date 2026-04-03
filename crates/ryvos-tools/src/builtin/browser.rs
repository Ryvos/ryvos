use std::sync::Arc;

use base64::Engine;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures::future::BoxFuture;
use futures::StreamExt;
use tokio::sync::Mutex;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

/// Shared browser session — lazily created on first use.
static BROWSER_SESSION: std::sync::OnceLock<Arc<BrowserSession>> = std::sync::OnceLock::new();

struct BrowserSession {
    browser: Mutex<Option<Browser>>,
    page: Mutex<Option<Page>>,
}

fn session() -> &'static Arc<BrowserSession> {
    BROWSER_SESSION.get_or_init(|| {
        Arc::new(BrowserSession {
            browser: Mutex::new(None),
            page: Mutex::new(None),
        })
    })
}

/// Find a Chrome/Chromium binary on the system.
fn find_chrome() -> Option<String> {
    // Allow env override
    if let Ok(path) = std::env::var("CHROME_PATH") {
        if std::path::Path::new(&path).exists() {
            return Some(path);
        }
    }

    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
    } else if cfg!(target_os = "windows") {
        vec![
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
        ]
    } else {
        // Linux
        vec![
            "/usr/bin/chromium-browser",
            "/usr/bin/chromium",
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/snap/bin/chromium",
        ]
    };

    for path in candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Ensure a browser and page are available, creating them if needed.
async fn ensure_page() -> Result<Page> {
    let sess = session();
    let mut browser_guard = sess.browser.lock().await;
    let mut page_guard = sess.page.lock().await;

    if let Some(ref page) = *page_guard {
        // Check if page is still usable by trying a simple eval
        match page.evaluate("1+1").await {
            Ok(_) => return Ok(page.clone()),
            Err(_) => {
                debug!("Browser page stale, recreating");
                *page_guard = None;
                *browser_guard = None;
            }
        }
    }

    let chrome_path = find_chrome().ok_or_else(|| RyvosError::ToolExecution {
        tool: "browser".into(),
        message: "Chrome/Chromium not found. Install Chrome or set CHROME_PATH env var.".into(),
    })?;

    debug!(chrome = %chrome_path, "Launching browser");

    let config = BrowserConfig::builder()
        .chrome_executable(chrome_path)
        .arg("--no-sandbox")
        .arg("--disable-gpu")
        .arg("--disable-dev-shm-usage")
        .arg("--disable-setuid-sandbox")
        .window_size(1280, 720)
        .build()
        .map_err(|e| RyvosError::ToolExecution {
            tool: "browser".into(),
            message: format!("Failed to build browser config: {e}"),
        })?;

    let (browser, mut handler) =
        Browser::launch(config)
            .await
            .map_err(|e| RyvosError::ToolExecution {
                tool: "browser".into(),
                message: format!("Failed to launch browser: {e}"),
            })?;

    // Spawn the handler loop
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });

    let page = browser
        .new_page("about:blank")
        .await
        .map_err(|e| RyvosError::ToolExecution {
            tool: "browser".into(),
            message: format!("Failed to create page: {e}"),
        })?;

    *browser_guard = Some(browser);
    let page_clone = page.clone();
    *page_guard = Some(page);

    Ok(page_clone)
}

// ── browser_navigate ────────────────────────────────────────────

pub struct BrowserNavigateTool;

impl Tool for BrowserNavigateTool {
    fn name(&self) -> &str {
        "browser_navigate"
    }

    fn description(&self) -> &str {
        "Navigate to a URL and return the page title and text content. Requires Chrome/Chromium installed on the system."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to"
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
            let url = input["url"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "browser_navigate".into(),
                    message: "Missing 'url' parameter".into(),
                })?
                .to_string();

            let page = ensure_page().await?;

            page.goto(&url)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "browser_navigate".into(),
                    message: format!("Navigation failed: {e}"),
                })?;

            // Wait for the page to be reasonably loaded
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let title = page
                .evaluate("document.title")
                .await
                .ok()
                .and_then(|v| v.into_value::<String>().ok())
                .unwrap_or_default();

            let text = page
                .evaluate("document.body?.innerText?.substring(0, 8000) || ''")
                .await
                .ok()
                .and_then(|v| v.into_value::<String>().ok())
                .unwrap_or_default();

            Ok(ToolResult::success(format!(
                "Title: {}\n\n{}",
                title,
                text.chars().take(8000).collect::<String>()
            )))
        })
    }

    fn timeout_secs(&self) -> u64 {
        60
    }

    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
}

// ── browser_screenshot ──────────────────────────────────────────

pub struct BrowserScreenshotTool;

impl Tool for BrowserScreenshotTool {
    fn name(&self) -> &str {
        "browser_screenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current page (or navigate to a URL first). Returns base64-encoded PNG."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Optional URL to navigate to before taking the screenshot"
                },
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector to screenshot a specific element"
                },
                "full_page": {
                    "type": "boolean",
                    "description": "Whether to capture the full page (default: false)"
                }
            }
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let page = ensure_page().await?;

            if let Some(url) = input["url"].as_str() {
                page.goto(url)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "browser_screenshot".into(),
                        message: format!("Navigation failed: {e}"),
                    })?;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }

            let png_data = if let Some(selector) = input["selector"].as_str() {
                let element =
                    page.find_element(selector)
                        .await
                        .map_err(|e| RyvosError::ToolExecution {
                            tool: "browser_screenshot".into(),
                            message: format!("Element not found '{}': {e}", selector),
                        })?;
                element
                    .screenshot(
                        chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png,
                    )
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "browser_screenshot".into(),
                        message: format!("Screenshot failed: {e}"),
                    })?
            } else {
                let params =
                    chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams::builder();
                let params = if input["full_page"].as_bool().unwrap_or(false) {
                    params.capture_beyond_viewport(true)
                } else {
                    params
                };
                page.screenshot(params.format(
                    chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png,
                ).build())
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "browser_screenshot".into(),
                    message: format!("Screenshot failed: {e}"),
                })?
            };

            let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);
            Ok(ToolResult::success(format!(
                "Screenshot captured ({} bytes). Base64 PNG:\n{}",
                png_data.len(),
                b64
            )))
        })
    }

    fn timeout_secs(&self) -> u64 {
        60
    }

    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
}

// ── browser_click ───────────────────────────────────────────────

pub struct BrowserClickTool;

impl Tool for BrowserClickTool {
    fn name(&self) -> &str {
        "browser_click"
    }

    fn description(&self) -> &str {
        "Click an element on the current page by CSS selector."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the element to click"
                }
            },
            "required": ["selector"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let selector = input["selector"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "browser_click".into(),
                    message: "Missing 'selector' parameter".into(),
                })?;

            let page = ensure_page().await?;

            let element =
                page.find_element(selector)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "browser_click".into(),
                        message: format!("Element not found '{}': {e}", selector),
                    })?;

            element
                .click()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "browser_click".into(),
                    message: format!("Click failed: {e}"),
                })?;

            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            Ok(ToolResult::success(format!(
                "Clicked element '{}'",
                selector
            )))
        })
    }

    fn timeout_secs(&self) -> u64 {
        30
    }

    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
}

// ── browser_type ────────────────────────────────────────────────

pub struct BrowserTypeTool;

impl Tool for BrowserTypeTool {
    fn name(&self) -> &str {
        "browser_type"
    }

    fn description(&self) -> &str {
        "Type text into an input element on the current page by CSS selector."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "CSS selector for the input element"
                },
                "text": {
                    "type": "string",
                    "description": "Text to type into the element"
                }
            },
            "required": ["selector", "text"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let selector = input["selector"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "browser_type".into(),
                    message: "Missing 'selector' parameter".into(),
                })?;

            let text = input["text"]
                .as_str()
                .ok_or_else(|| RyvosError::ToolExecution {
                    tool: "browser_type".into(),
                    message: "Missing 'text' parameter".into(),
                })?;

            let page = ensure_page().await?;

            let element =
                page.find_element(selector)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "browser_type".into(),
                        message: format!("Element not found '{}': {e}", selector),
                    })?;

            element
                .click()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "browser_type".into(),
                    message: format!("Focus failed: {e}"),
                })?;

            element
                .type_str(text)
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "browser_type".into(),
                    message: format!("Typing failed: {e}"),
                })?;

            Ok(ToolResult::success(format!(
                "Typed {} chars into '{}'",
                text.len(),
                selector
            )))
        })
    }

    fn timeout_secs(&self) -> u64 {
        30
    }

    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
}

// ── browser_extract ─────────────────────────────────────────────

pub struct BrowserExtractTool;

impl Tool for BrowserExtractTool {
    fn name(&self) -> &str {
        "browser_extract"
    }

    fn description(&self) -> &str {
        "Extract text content from the current page, optionally scoped to a CSS selector."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector to scope extraction (defaults to full page body)"
                }
            }
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let page = ensure_page().await?;

            let text = if let Some(selector) = input["selector"].as_str() {
                let js = format!(
                    "document.querySelector('{}')?.innerText?.substring(0, 8000) || 'Element not found'",
                    selector.replace('\'', "\\'")
                );
                page.evaluate(js)
                    .await
                    .ok()
                    .and_then(|v| v.into_value::<String>().ok())
                    .unwrap_or_else(|| "Failed to extract text".into())
            } else {
                page.evaluate("document.body?.innerText?.substring(0, 8000) || ''")
                    .await
                    .ok()
                    .and_then(|v| v.into_value::<String>().ok())
                    .unwrap_or_default()
            };

            Ok(ToolResult::success(text))
        })
    }

    fn timeout_secs(&self) -> u64 {
        30
    }

    fn tier(&self) -> SecurityTier {
        SecurityTier::T3
    }
}

/// Register all browser tools into a tool registry.
pub fn register_browser_tools(registry: &mut crate::ToolRegistry) {
    registry.register(BrowserNavigateTool);
    registry.register(BrowserScreenshotTool);
    registry.register(BrowserClickTool);
    registry.register(BrowserTypeTool);
    registry.register(BrowserExtractTool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_chrome_with_env() {
        // Test that CHROME_PATH override works.
        // Note: env::set_var is not thread-safe, and on CI runners Chrome may
        // already be installed. We use a unique temp file to avoid collisions.
        let tmp = std::env::temp_dir().join("ryvos_chrome_test_bin");
        std::fs::write(&tmp, "fake chrome").ok();
        let path_str = tmp.to_string_lossy().to_string();

        unsafe { std::env::set_var("CHROME_PATH", &path_str) };
        let result = find_chrome();
        unsafe { std::env::remove_var("CHROME_PATH") };
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(result, Some(path_str));
    }

    #[test]
    fn test_find_chrome_nonexistent_env() {
        unsafe { std::env::set_var("CHROME_PATH", "/nonexistent/path/chrome") };
        // Should fall through to system paths since file doesn't exist
        let _result = find_chrome();
        unsafe { std::env::remove_var("CHROME_PATH") };
    }
}
