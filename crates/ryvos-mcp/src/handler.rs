use std::future::Future;

use tokio::sync::broadcast;
use tracing::{debug, warn};

use rmcp::handler::client::ClientHandler;
use rmcp::model::*;
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::ErrorData as McpError;
use rmcp::RoleClient;

/// Events emitted by the MCP notification handler.
#[derive(Debug, Clone)]
pub enum McpEvent {
    ToolsChanged {
        server: String,
    },
    ResourcesChanged {
        server: String,
    },
    PromptsChanged {
        server: String,
    },
    ResourceUpdated {
        server: String,
        uri: String,
    },
    LogMessage {
        server: String,
        level: String,
        message: String,
    },
}

/// Custom MCP client handler that processes server notifications
/// and optionally supports sampling (server-to-client LLM requests).
pub struct RyvosClientHandler {
    server_name: String,
    event_tx: broadcast::Sender<McpEvent>,
}

impl RyvosClientHandler {
    pub fn new(server_name: &str, event_tx: broadcast::Sender<McpEvent>) -> Self {
        Self {
            server_name: server_name.to_string(),
            event_tx,
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl ClientHandler for RyvosClientHandler {
    fn on_tool_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async {
            debug!(server = %self.server_name, "MCP tools/list_changed notification");
            let _ = self.event_tx.send(McpEvent::ToolsChanged {
                server: self.server_name.clone(),
            });
        }
    }

    fn on_resource_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async {
            debug!(server = %self.server_name, "MCP resources/list_changed notification");
            let _ = self.event_tx.send(McpEvent::ResourcesChanged {
                server: self.server_name.clone(),
            });
        }
    }

    fn on_prompt_list_changed(
        &self,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async {
            debug!(server = %self.server_name, "MCP prompts/list_changed notification");
            let _ = self.event_tx.send(McpEvent::PromptsChanged {
                server: self.server_name.clone(),
            });
        }
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async move {
            debug!(server = %self.server_name, uri = %params.uri, "MCP resource updated");
            let _ = self.event_tx.send(McpEvent::ResourceUpdated {
                server: self.server_name.clone(),
                uri: params.uri.to_string(),
            });
        }
    }

    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async move {
            let level = format!("{:?}", params.level);
            let message = params.data.to_string();
            debug!(server = %self.server_name, level = %level, "MCP log: {}", message);
            let _ = self.event_tx.send(McpEvent::LogMessage {
                server: self.server_name.clone(),
                level,
                message,
            });
        }
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _ctx: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        async move {
            debug!(
                server = %self.server_name,
                progress = params.progress,
                total = ?params.total,
                message = ?params.message,
                "MCP progress"
            );
        }
    }

    fn create_message(
        &self,
        _params: CreateMessageRequestParams,
        _ctx: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<CreateMessageResult, McpError>> + Send + '_ {
        async {
            // Sampling is not yet implemented — return method_not_found
            warn!(server = %self.server_name, "Server requested sampling (create_message) — not yet supported");
            Err(McpError::method_not_found::<CreateMessageRequestMethod>())
        }
    }

    fn get_info(&self) -> ClientInfo {
        ClientInfo {
            meta: None,
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "ryvos".into(),
                title: None,
                version: env!("CARGO_PKG_VERSION").into(),
                description: None,
                icons: None,
                website_url: None,
            },
        }
    }
}
