use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::security::{
    ApprovalDecision, ApprovalRequest, DangerousPatternMatcher, GateDecision, SecurityPolicy,
    SecurityTier,
};
use ryvos_core::traits::Tool;
use ryvos_core::types::{AgentEvent, ToolContext, ToolDefinition, ToolResult};
use ryvos_tools::ToolRegistry;

use crate::approval::ApprovalBroker;

/// SecurityGate — intercepts tool calls between agent loop and registry.
pub struct SecurityGate {
    policy: SecurityPolicy,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    broker: Arc<ApprovalBroker>,
    event_bus: Arc<EventBus>,
    matcher: DangerousPatternMatcher,
}

impl SecurityGate {
    pub fn new(
        policy: SecurityPolicy,
        tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
        broker: Arc<ApprovalBroker>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        let matcher = DangerousPatternMatcher::new(&policy.dangerous_patterns);
        Self {
            policy,
            tools,
            broker,
            event_bus,
            matcher,
        }
    }

    /// Main entry point — replaces direct tools.execute() calls.
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult> {
        let tool = {
            let tools = self.tools.read().await;
            tools
                .get(name)
                .ok_or_else(|| RyvosError::ToolNotFound(name.to_string()))?
        };

        let base_tier = tool.tier();
        let effective = self.effective_tier(name, base_tier, &input);

        match self.policy.decide(effective, name) {
            GateDecision::Allow => self.execute_tool_direct(&tool, name, input, ctx).await,
            GateDecision::Deny => {
                let reason = format!(
                    "Security policy denies tier {} for tool '{}'",
                    effective, name
                );
                self.event_bus.publish(AgentEvent::ToolBlocked {
                    name: name.to_string(),
                    tier: effective,
                    reason: reason.clone(),
                });
                Err(RyvosError::ToolBlocked {
                    tool: name.to_string(),
                    tier: effective.to_string(),
                })
            }
            GateDecision::NeedsApproval => {
                let input_summary = summarize_input(name, &input);
                let req = ApprovalRequest {
                    id: Uuid::new_v4().to_string(),
                    tool_name: name.to_string(),
                    tier: effective,
                    input_summary,
                    session_id: ctx.session_id.to_string(),
                    timestamp: Utc::now(),
                };

                let rx = self.broker.request(req).await;
                let timeout = Duration::from_secs(self.policy.approval_timeout_secs);

                match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(ApprovalDecision::Approved)) => {
                        self.execute_tool_direct(&tool, name, input, ctx).await
                    }
                    Ok(Ok(ApprovalDecision::Denied { reason })) => {
                        Err(RyvosError::ApprovalDenied {
                            tool: name.to_string(),
                            reason,
                        })
                    }
                    Ok(Err(_)) => {
                        // Sender dropped (broker cleaned up)
                        Err(RyvosError::ApprovalTimeout {
                            tool: name.to_string(),
                        })
                    }
                    Err(_) => {
                        // Timeout elapsed
                        Err(RyvosError::ApprovalTimeout {
                            tool: name.to_string(),
                        })
                    }
                }
            }
        }
    }

    /// Execute a tool directly using an already-resolved Arc<dyn Tool>.
    async fn execute_tool_direct(
        &self,
        tool: &Arc<dyn Tool>,
        name: &str,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult> {
        let timeout = Duration::from_secs(tool.timeout_secs());
        match tokio::time::timeout(timeout, tool.execute(input, ctx)).await {
            Ok(result) => result,
            Err(_) => Err(RyvosError::ToolTimeout {
                tool: name.to_string(),
                timeout_secs: tool.timeout_secs(),
            }),
        }
    }

    /// Get effective tier (base + input inspection for bash).
    fn effective_tier(
        &self,
        name: &str,
        base: SecurityTier,
        input: &serde_json::Value,
    ) -> SecurityTier {
        // For bash-like tools: check dangerous patterns → escalate to T4
        if name == "bash" {
            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                if self.matcher.is_dangerous(cmd).is_some() {
                    return SecurityTier::T4;
                }
            }
        }
        base
    }

    /// Get tool definitions (delegates to registry).
    pub async fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.read().await.definitions()
    }

    /// Get a reference to the underlying tool registry lock.
    pub fn tools_lock(&self) -> &Arc<tokio::sync::RwLock<ToolRegistry>> {
        &self.tools
    }

    /// Get the current security policy.
    pub fn policy(&self) -> &SecurityPolicy {
        &self.policy
    }
}

/// Summarize tool input for display in approval prompts.
fn summarize_input(tool_name: &str, input: &serde_json::Value) -> String {
    match tool_name {
        "bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown command>")
            .to_string(),
        "write" | "edit" => input
            .get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown file>")
            .to_string(),
        "web_search" => input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("<unknown query>")
            .to_string(),
        "spawn_agent" => input
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(|s| {
                if s.len() > 80 {
                    format!("{}...", &s[..80])
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_else(|| "<unknown prompt>".to_string()),
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 120 {
                format!("{}...", &s[..120])
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::security::SecurityPolicy;
    use ryvos_core::types::SessionId;
    use ryvos_tools::ToolRegistry;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: SessionId::new(),
            working_dir: std::env::temp_dir(),
            store: None,
            agent_spawner: None,
            sandbox_config: None,
        }
    }

    fn make_gate(policy: SecurityPolicy) -> SecurityGate {
        let tools = Arc::new(tokio::sync::RwLock::new(ToolRegistry::with_builtins()));
        let event_bus = Arc::new(EventBus::default());
        let broker = Arc::new(ApprovalBroker::new(event_bus.clone()));
        SecurityGate::new(policy, tools, broker, event_bus)
    }

    #[tokio::test]
    async fn auto_approve_t0() {
        let gate = make_gate(SecurityPolicy::default());
        let input = serde_json::json!({"file_path": "/tmp/test.txt"});
        let result = gate.execute("read", input, test_ctx()).await;
        // read is T0, auto-approved — might fail on file not found, but won't be blocked
        assert!(
            result.is_ok() || matches!(result, Err(RyvosError::ToolExecution { .. })),
            "T0 tool should not be blocked"
        );
    }

    #[tokio::test]
    async fn needs_approval_t2() {
        let input = serde_json::json!({"command": "echo hello"});
        // bash is T2 > auto_approve T1, needs approval. No one will approve → timeout.
        // Use a very short timeout to speed up the test.
        let policy = SecurityPolicy {
            approval_timeout_secs: 0,
            ..Default::default()
        };
        let gate = make_gate(policy);
        let result = gate.execute("bash", input, test_ctx()).await;
        assert!(matches!(result, Err(RyvosError::ApprovalTimeout { .. })));
    }

    #[tokio::test]
    async fn block_above_deny() {
        let policy = SecurityPolicy {
            deny_above: Some(SecurityTier::T1),
            ..Default::default()
        };
        let gate = make_gate(policy);
        let input = serde_json::json!({"command": "echo hello"});
        let result = gate.execute("bash", input, test_ctx()).await;
        assert!(matches!(result, Err(RyvosError::ToolBlocked { .. })));
    }

    #[tokio::test]
    async fn escalate_bash_rm() {
        let policy = SecurityPolicy {
            deny_above: Some(SecurityTier::T3),
            ..Default::default()
        };
        let gate = make_gate(policy);
        let input = serde_json::json!({"command": "rm -rf /tmp/data"});
        // bash base T2, but rm -rf escalates to T4, which is > deny_above T3 → blocked
        let result = gate.execute("bash", input, test_ctx()).await;
        assert!(matches!(result, Err(RyvosError::ToolBlocked { .. })));
    }

    #[tokio::test]
    async fn tool_override() {
        let mut policy = SecurityPolicy::default();
        // Override bash to T1 (auto-approved)
        policy
            .tool_overrides
            .insert("bash".to_string(), SecurityTier::T1);
        let gate = make_gate(policy);
        let input = serde_json::json!({"command": "echo hello"});
        // Now bash should be auto-approved at T1 via policy override
        // The effective_tier still returns T2 from the tool, but policy.decide checks overrides
        let result = gate.execute("bash", input, test_ctx()).await;
        // Should succeed (auto-approved) — echo command runs and returns
        assert!(result.is_ok());
    }
}
