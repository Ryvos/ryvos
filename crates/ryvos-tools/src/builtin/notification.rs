use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct NotificationSendTool;

#[derive(Deserialize)]
struct NotificationInput {
    message: String,
    #[serde(default = "default_channel")]
    channel: String,
}
fn default_channel() -> String {
    "log".into()
}

impl Tool for NotificationSendTool {
    fn name(&self) -> &str {
        "notification_send"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn description(&self) -> &str {
        "Send a notification. Currently logs to a notification file."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "Notification message" },
                "channel": { "type": "string", "description": "Target channel (default: log)" }
            },
            "required": ["message"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: NotificationInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let notif_dir = std::path::PathBuf::from(&home)
                .join(".ryvos")
                .join("notifications");
            tokio::fs::create_dir_all(&notif_dir).await.ok();

            let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let entry = format!("[{}] [{}] {}\n", timestamp, p.channel, p.message);

            let log_file = notif_dir.join("notifications.log");
            let existing = tokio::fs::read_to_string(&log_file)
                .await
                .unwrap_or_default();
            tokio::fs::write(&log_file, format!("{}{}", existing, entry))
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "notification_send".into(),
                    message: e.to_string(),
                })?;

            tracing::info!(channel = %p.channel, "Notification sent: {}", p.message);
            Ok(ToolResult::success(format!(
                "Notification sent via {}: {}",
                p.channel, p.message
            )))
        })
    }
}
