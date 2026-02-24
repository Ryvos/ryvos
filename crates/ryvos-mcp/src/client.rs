use std::collections::HashMap;

use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info, warn};

use rmcp::model::{
    CallToolRequestParams, GetPromptRequestParams, Prompt, ReadResourceRequestParams,
    Resource, ResourceContents, SubscribeRequestParams, Tool as McpTool,
};
use rmcp::service::RunningService;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransport;
use rmcp::{RoleClient, ServiceExt};

use ryvos_core::config::{McpServerConfig, McpTransport};
use ryvos_core::error::RyvosError;

use crate::handler::{McpEvent, RyvosClientHandler};

type McpConnection = RunningService<RoleClient, RyvosClientHandler>;

/// Manages connections to multiple MCP servers.
pub struct McpClientManager {
    connections: Mutex<HashMap<String, McpConnection>>,
    server_configs: Mutex<HashMap<String, McpServerConfig>>,
    event_tx: broadcast::Sender<McpEvent>,
}

impl Default for McpClientManager {
    fn default() -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            connections: Mutex::new(HashMap::new()),
            server_configs: Mutex::new(HashMap::new()),
            event_tx,
        }
    }
}

impl McpClientManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to MCP events (tools_changed, resources_changed, etc.).
    pub fn subscribe_events(&self) -> broadcast::Receiver<McpEvent> {
        self.event_tx.subscribe()
    }

    /// Connect to an MCP server.
    pub async fn connect(
        &self,
        name: &str,
        config: &McpServerConfig,
    ) -> Result<(), RyvosError> {
        let handler = RyvosClientHandler::new(name, self.event_tx.clone());

        let client = match &config.transport {
            McpTransport::Stdio { command, args, env } => {
                let mut cmd = tokio::process::Command::new(command);
                cmd.args(args);
                for (k, v) in env {
                    cmd.env(k, v);
                }

                let transport = rmcp::transport::TokioChildProcess::new(cmd)
                    .map_err(|e| {
                        RyvosError::Mcp(format!("Failed to spawn {}: {}", command, e))
                    })?;

                handler
                    .serve(transport)
                    .await
                    .map_err(|e| {
                        RyvosError::Mcp(format!(
                            "Failed to initialize MCP client for {}: {}",
                            name, e
                        ))
                    })?
            }
            McpTransport::Sse { url } => {
                let transport = StreamableHttpClientTransport::from_uri(url.as_str());

                <RyvosClientHandler as ServiceExt<RoleClient>>::serve(handler, transport)
                    .await
                    .map_err(|e| {
                        RyvosError::Mcp(format!(
                            "MCP init for '{}' failed: {}",
                            name, e
                        ))
                    })?
            }
        };

        info!(server = %name, "MCP server connected");

        self.connections
            .lock()
            .await
            .insert(name.to_string(), client);
        self.server_configs
            .lock()
            .await
            .insert(name.to_string(), config.clone());
        Ok(())
    }

    /// Attempt to reconnect to a server using its stored config.
    pub async fn reconnect(&self, server_name: &str) -> Result<(), RyvosError> {
        let config = {
            let configs = self.server_configs.lock().await;
            configs
                .get(server_name)
                .cloned()
                .ok_or_else(|| {
                    RyvosError::Mcp(format!(
                        "No stored config for server '{}'",
                        server_name
                    ))
                })?
        };

        // Remove old connection
        {
            let mut conns = self.connections.lock().await;
            if let Some(mut old) = conns.remove(server_name) {
                let _ = old.close().await;
            }
        }

        self.connect(server_name, &config).await
    }

    /// Check if a server connection is still alive.
    pub async fn is_connected(&self, server_name: &str) -> bool {
        let conns = self.connections.lock().await;
        conns
            .get(server_name)
            .map(|c| !c.is_closed())
            .unwrap_or(false)
    }

    /// List all connected server names.
    pub async fn connected_servers(&self) -> Vec<String> {
        let conns = self.connections.lock().await;
        conns.keys().cloned().collect()
    }

    /// List all configured server names (including disconnected).
    pub async fn configured_servers(&self) -> Vec<String> {
        let configs = self.server_configs.lock().await;
        configs.keys().cloned().collect()
    }

    /// Get the stored config for a server.
    pub async fn get_config(&self, server_name: &str) -> Option<McpServerConfig> {
        let configs = self.server_configs.lock().await;
        configs.get(server_name).cloned()
    }

    // ---- Tools ----

    /// List tools from a connected server.
    pub async fn list_tools(
        &self,
        server_name: &str,
    ) -> Result<Vec<McpTool>, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let tools = client.list_all_tools().await.map_err(|e| {
            RyvosError::Mcp(format!(
                "Failed to list tools from '{}': {}",
                server_name, e
            ))
        })?;

        debug!(server = %server_name, count = tools.len(), "Listed MCP tools");
        Ok(tools)
    }

    /// Call a tool on a connected server, with automatic reconnect on transport failure.
    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<String, RyvosError> {
        let result = self
            .call_tool_inner(server_name, tool_name, arguments.clone())
            .await;

        // If transport closed, attempt one reconnect
        if let Err(ref e) = result {
            let err_str = e.to_string();
            if err_str.contains("closed") || err_str.contains("Transport") {
                warn!(server = %server_name, "MCP transport closed, attempting reconnect");
                if self.reconnect(server_name).await.is_ok() {
                    return self
                        .call_tool_inner(server_name, tool_name, arguments)
                        .await;
                }
            }
        }

        result
    }

    async fn call_tool_inner(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<String, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let params = CallToolRequestParams {
            name: tool_name.to_string().into(),
            arguments,
            meta: None,
            task: None,
        };

        let result = client.call_tool(params).await.map_err(|e| {
            RyvosError::Mcp(format!(
                "Tool call '{}.{}' failed: {}",
                server_name, tool_name, e
            ))
        })?;

        // Convert result content to string
        let content: Vec<String> = result
            .content
            .iter()
            .map(|c| match c.raw {
                rmcp::model::RawContent::Text(ref t) => t.text.to_string(),
                _ => format!("{:?}", c.raw),
            })
            .collect();

        Ok(content.join("\n"))
    }

    // ---- Resources ----

    /// List resources from a connected server.
    pub async fn list_resources(
        &self,
        server_name: &str,
    ) -> Result<Vec<Resource>, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let resources =
            client.list_all_resources().await.map_err(|e| {
                RyvosError::Mcp(format!(
                    "Failed to list resources from '{}': {}",
                    server_name, e
                ))
            })?;

        debug!(server = %server_name, count = resources.len(), "Listed MCP resources");
        Ok(resources)
    }

    /// Read a resource by URI from a connected server.
    pub async fn read_resource(
        &self,
        server_name: &str,
        uri: &str,
    ) -> Result<String, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let params = ReadResourceRequestParams {
            uri: uri.to_string(),
            meta: None,
        };

        let result =
            client.read_resource(params).await.map_err(|e| {
                RyvosError::Mcp(format!(
                    "Failed to read resource '{}' from '{}': {}",
                    uri, server_name, e
                ))
            })?;

        let text: Vec<String> = result
            .contents
            .iter()
            .map(|c| match c {
                ResourceContents::TextResourceContents { text, .. } => {
                    text.clone()
                }
                ResourceContents::BlobResourceContents { blob, .. } => {
                    format!("[blob: {} bytes]", blob.len())
                }
            })
            .collect();

        Ok(text.join("\n"))
    }

    /// Subscribe to resource changes on a server.
    pub async fn subscribe_resource(
        &self,
        server_name: &str,
        uri: &str,
    ) -> Result<(), RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let params = SubscribeRequestParams {
            uri: uri.to_string(),
            meta: None,
        };

        client.subscribe(params).await.map_err(|e| {
            RyvosError::Mcp(format!(
                "Failed to subscribe to '{}' on '{}': {}",
                uri, server_name, e
            ))
        })?;

        debug!(server = %server_name, uri = %uri, "Subscribed to MCP resource");
        Ok(())
    }

    // ---- Prompts ----

    /// List prompts from a connected server.
    pub async fn list_prompts(
        &self,
        server_name: &str,
    ) -> Result<Vec<Prompt>, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let prompts =
            client.list_all_prompts().await.map_err(|e| {
                RyvosError::Mcp(format!(
                    "Failed to list prompts from '{}': {}",
                    server_name, e
                ))
            })?;

        debug!(server = %server_name, count = prompts.len(), "Listed MCP prompts");
        Ok(prompts)
    }

    /// Get a specific prompt's content from a server.
    pub async fn get_prompt(
        &self,
        server_name: &str,
        prompt_name: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<Vec<PromptMessage>, RyvosError> {
        let conns = self.connections.lock().await;
        let client = conns.get(server_name).ok_or_else(|| {
            RyvosError::Mcp(format!("Server '{}' not connected", server_name))
        })?;

        let params = GetPromptRequestParams {
            name: prompt_name.to_string(),
            arguments: arguments.map(|m| m.into_iter().collect()),
            meta: None,
        };

        let result = client.get_prompt(params).await.map_err(|e| {
            RyvosError::Mcp(format!(
                "Failed to get prompt '{}' from '{}': {}",
                prompt_name, server_name, e
            ))
        })?;

        Ok(result.messages)
    }

    // ---- Connection management ----

    /// Disconnect from a specific server.
    pub async fn disconnect(&self, server_name: &str) {
        let mut conns = self.connections.lock().await;
        if let Some(mut client) = conns.remove(server_name) {
            let _ = client.close().await;
            info!(server = %server_name, "MCP server disconnected");
        }
    }

    /// Disconnect from all servers.
    pub async fn disconnect_all(&self) {
        let mut conns = self.connections.lock().await;
        let names: Vec<String> = conns.keys().cloned().collect();
        for name in names {
            if let Some(mut client) = conns.remove(&name) {
                let _ = client.close().await;
                info!(server = %name, "MCP server disconnected");
            }
        }
    }
}

use rmcp::model::PromptMessage;
