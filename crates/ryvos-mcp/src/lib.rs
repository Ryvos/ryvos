mod bridge;
mod client;
mod handler;
mod resource_tool;

pub use bridge::register_mcp_tools;
pub use client::McpClientManager;
pub use handler::{McpEvent, RyvosClientHandler};
pub use resource_tool::McpReadResourceTool;

// Re-export prompt types for consumers that don't depend on rmcp directly
pub use rmcp::model::{PromptMessage, PromptMessageContent, PromptMessageRole};

use std::sync::Arc;

use tracing::debug;

use ryvos_core::config::McpServerConfig;
use ryvos_core::error::RyvosError;
use ryvos_tools::ToolRegistry;

/// Connect to an MCP server and register its tools into the registry.
/// Returns the number of tools registered.
pub async fn connect_and_register(
    manager: &Arc<McpClientManager>,
    server_name: &str,
    config: &McpServerConfig,
    registry: &mut ToolRegistry,
) -> Result<usize, RyvosError> {
    manager.connect(server_name, config).await?;

    let tools = manager.list_tools(server_name).await?;
    let count = tools.len();

    bridge::register_mcp_tools(
        registry,
        manager,
        server_name,
        &tools,
        config.timeout_secs,
        config.tier_override.as_deref(),
    );

    Ok(count)
}

/// Re-fetch tools from a server and update the registry.
/// Unregisters all existing mcp__{server}__* tools first, then re-registers.
/// Returns the new tool count.
pub async fn refresh_tools(
    manager: &Arc<McpClientManager>,
    server_name: &str,
    registry: &mut ToolRegistry,
) -> Result<usize, RyvosError> {
    // Unregister all existing tools for this server
    let prefix = format!("mcp__{}__", server_name);
    let to_remove: Vec<String> = registry
        .list()
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .map(|s| s.to_string())
        .collect();

    for name in &to_remove {
        registry.unregister(name);
    }
    debug!(server = %server_name, removed = to_remove.len(), "Unregistered old MCP tools");

    // Re-fetch and register
    let tools = manager.list_tools(server_name).await?;
    let count = tools.len();

    let config = manager.get_config(server_name).await;
    let (timeout, tier) = config
        .as_ref()
        .map(|c| (c.timeout_secs, c.tier_override.as_deref()))
        .unwrap_or((120, None));

    bridge::register_mcp_tools(registry, manager, server_name, &tools, timeout, tier);

    debug!(server = %server_name, new_count = count, "Refreshed MCP tools");
    Ok(count)
}
