use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::security::{
    summarize_input, tool_has_side_effects, ApprovalDecision, ApprovalRequest, SecurityPolicy,
};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolDefinition, ToolResult};
use ryvos_tools::ToolRegistry;

use crate::approval::ApprovalBroker;
use crate::audit::{AuditEntry, AuditTrail};
use crate::safety_memory::{assess_outcome, SafetyMemory, SafetyOutcome};

/// SecurityGate — passthrough that logs, learns, and optionally pauses.
///
/// **No tool is ever blocked.** The gate:
/// 1. Logs the tool call to the audit trail
/// 2. Checks safety memory for relevant lessons (informational)
/// 3. If user configured `pause_before`, waits for acknowledgment
/// 4. Executes the tool — always
/// 5. Post-action: assesses outcome and records lessons
pub struct SecurityGate {
    policy: SecurityPolicy,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    broker: Arc<ApprovalBroker>,
    #[allow(dead_code)]
    event_bus: Arc<EventBus>,
    safety_memory: Option<Arc<SafetyMemory>>,
    audit_trail: Option<Arc<AuditTrail>>,
}

impl SecurityGate {
    pub fn new(
        policy: SecurityPolicy,
        tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
        broker: Arc<ApprovalBroker>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            policy,
            tools,
            broker,
            event_bus,
            safety_memory: None,
            audit_trail: None,
        }
    }

    /// Set the safety memory store for self-learning.
    pub fn set_safety_memory(&mut self, memory: Arc<SafetyMemory>) {
        self.safety_memory = Some(memory);
    }

    /// Set the audit trail for persistent logging.
    pub fn set_audit_trail(&mut self, trail: Arc<AuditTrail>) {
        self.audit_trail = Some(trail);
    }

    /// Main entry point — always executes the tool.
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

        // 1. Log to audit trail (pre-execution)
        let input_summary = summarize_input(name, &input);
        if let Some(ref trail) = self.audit_trail {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                session_id: ctx.session_id.to_string(),
                tool_name: name.to_string(),
                input_summary: input_summary.clone(),
                output_summary: String::new(), // Filled post-execution
                safety_reasoning: None,
                outcome: SafetyOutcome::Harmless,
                lessons_available: vec![],
            };
            if let Err(e) = trail.log_tool_call(&entry).await {
                debug!(error = %e, "Failed to log pre-execution audit entry");
            }
        }

        // 2. Check safety memory (informational, never blocking)
        let mut lesson_ids = Vec::new();
        if let Some(ref memory) = self.safety_memory {
            if let Ok(lessons) = memory.relevant_lessons(name, 3).await {
                if !lessons.is_empty() {
                    info!(
                        tool = name,
                        lesson_count = lessons.len(),
                        "Safety memory: relevant lessons available"
                    );
                    lesson_ids = lessons.iter().map(|l| l.id.clone()).collect();
                }
            }
        }

        // 3. Optional soft checkpoint (pause_before)
        if self.policy.should_pause(name) && tool_has_side_effects(name) {
            let req = ApprovalRequest {
                id: Uuid::new_v4().to_string(),
                tool_name: name.to_string(),
                tier: tool.tier(),
                input_summary: input_summary.clone(),
                session_id: ctx.session_id.to_string(),
                timestamp: Utc::now(),
            };

            let rx = self.broker.request(req).await;
            let timeout = Duration::from_secs(self.policy.approval_timeout_secs);

            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(ApprovalDecision::Denied { reason })) => {
                    // User explicitly denied — this is the ONLY way a tool gets stopped
                    warn!(tool = name, reason = %reason, "User denied soft checkpoint");
                    return Err(RyvosError::ApprovalDenied {
                        tool: name.to_string(),
                        reason,
                    });
                }
                Ok(Ok(ApprovalDecision::Approved)) => {
                    debug!(tool = name, "Soft checkpoint approved");
                }
                Ok(Err(_)) | Err(_) => {
                    // Timeout or channel dropped — proceed anyway (no blocking)
                    debug!(tool = name, "Soft checkpoint timed out — proceeding");
                }
            }
        }

        // 4. Execute — always
        let result = self
            .execute_tool_direct(&tool, name, input.clone(), ctx.clone())
            .await;

        // 5. Post-action: assess outcome and learn
        match &result {
            Ok(tool_result) => {
                let outcome =
                    assess_outcome(name, &input, &tool_result.content, tool_result.is_error);

                // Record to audit trail (post-execution)
                if let Some(ref trail) = self.audit_trail {
                    let output_preview: String = tool_result.content.chars().take(200).collect();
                    let entry = AuditEntry {
                        timestamp: Utc::now(),
                        session_id: ctx.session_id.to_string(),
                        tool_name: name.to_string(),
                        input_summary: input_summary.clone(),
                        output_summary: output_preview,
                        safety_reasoning: None,
                        outcome: outcome.clone(),
                        lessons_available: lesson_ids.clone(),
                    };
                    if let Err(e) = trail.log_tool_call(&entry).await {
                        debug!(error = %e, "Failed to log post-execution audit entry");
                    }
                }

                // Record safety lesson for incidents
                if let Some(ref memory) = self.safety_memory {
                    match &outcome {
                        SafetyOutcome::Incident {
                            what_happened,
                            severity,
                        } => {
                            let lesson = crate::safety_memory::SafetyLesson {
                                id: Uuid::new_v4().to_string(),
                                timestamp: Utc::now(),
                                action: format!("{}({})", name, input_summary),
                                outcome: outcome.clone(),
                                reflection: format!(
                                    "Tool {} resulted in {:?} incident: {}",
                                    name, severity, what_happened
                                ),
                                principle_violated: None,
                                corrective_rule: format!(
                                    "Be cautious with {} — verify preconditions before executing",
                                    name
                                ),
                                confidence: match severity {
                                    crate::safety_memory::Severity::Critical => 1.0,
                                    crate::safety_memory::Severity::High => 0.95,
                                    crate::safety_memory::Severity::Medium => 0.8,
                                    crate::safety_memory::Severity::Low => 0.6,
                                },
                                times_applied: 0,
                            };
                            if let Err(e) = memory.record_lesson(&lesson).await {
                                debug!(error = %e, "Failed to record safety lesson");
                            }
                        }
                        SafetyOutcome::NearMiss { .. } => {
                            // Reinforce relevant existing lessons
                            for id in &lesson_ids {
                                if let Err(e) = memory.reinforce(id).await {
                                    debug!(error = %e, lesson_id = %id, "Failed to reinforce lesson");
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                // Record execution error in audit trail
                if let Some(ref trail) = self.audit_trail {
                    let entry = AuditEntry {
                        timestamp: Utc::now(),
                        session_id: ctx.session_id.to_string(),
                        tool_name: name.to_string(),
                        input_summary,
                        output_summary: format!("ERROR: {}", e),
                        safety_reasoning: None,
                        outcome: SafetyOutcome::Incident {
                            what_happened: e.to_string(),
                            severity: crate::safety_memory::Severity::Low,
                        },
                        lessons_available: lesson_ids,
                    };
                    if let Err(e) = trail.log_tool_call(&entry).await {
                        debug!(error = %e, "Failed to log error audit entry");
                    }
                }
            }
        }

        result
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

    /// Get the safety memory (if configured).
    pub fn safety_memory(&self) -> Option<&Arc<SafetyMemory>> {
        self.safety_memory.as_ref()
    }

    /// Get the audit trail (if configured).
    pub fn audit_trail(&self) -> Option<&Arc<AuditTrail>> {
        self.audit_trail.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ryvos_core::security::{SecurityPolicy, SecurityTier};
    use ryvos_core::types::SessionId;
    use ryvos_tools::ToolRegistry;

    fn test_ctx() -> ToolContext {
        ToolContext {
            session_id: SessionId::new(),
            working_dir: std::env::temp_dir(),
            store: None,
            agent_spawner: None,
            sandbox_config: None,
            config_path: None,
            viking_client: None,
        }
    }

    fn make_gate(policy: SecurityPolicy) -> SecurityGate {
        let tools = Arc::new(tokio::sync::RwLock::new(ToolRegistry::with_builtins()));
        let event_bus = Arc::new(EventBus::default());
        let broker = Arc::new(ApprovalBroker::new(event_bus.clone()));
        SecurityGate::new(policy, tools, broker, event_bus)
    }

    #[tokio::test]
    async fn all_tools_execute_freely() {
        // With the new passthrough gate, even "dangerous" commands execute
        let gate = make_gate(SecurityPolicy::default());
        let input = serde_json::json!({"command": "echo hello"});
        let result = gate.execute("bash", input, test_ctx()).await;
        // bash should execute successfully (not blocked)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn read_tool_executes() {
        let gate = make_gate(SecurityPolicy::default());
        let input = serde_json::json!({"file_path": "/tmp/test.txt"});
        let result = gate.execute("read", input, test_ctx()).await;
        // read is safe — might fail on file not found, but won't be blocked
        assert!(
            result.is_ok() || matches!(result, Err(RyvosError::ToolExecution { .. })),
            "T0 tool should never be blocked"
        );
    }

    #[tokio::test]
    async fn no_blocking_on_any_tier() {
        // Even with old-style config, tools are never blocked
        let policy = SecurityPolicy {
            deny_above: Some(SecurityTier::T1), // Would have blocked T2+ before
            ..Default::default()
        };
        let gate = make_gate(policy);
        let input = serde_json::json!({"command": "echo hello"});
        // bash (T2) would have been blocked before — now it executes
        let result = gate.execute("bash", input, test_ctx()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn tool_not_found_still_errors() {
        let gate = make_gate(SecurityPolicy::default());
        let result = gate
            .execute("nonexistent_tool", serde_json::Value::Null, test_ctx())
            .await;
        assert!(matches!(result, Err(RyvosError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn pause_before_timeout_proceeds() {
        // If pause_before is set but no one approves, it proceeds after timeout
        let policy = SecurityPolicy {
            pause_before: vec!["bash".to_string()],
            approval_timeout_secs: 0, // Instant timeout
            ..Default::default()
        };
        let gate = make_gate(policy);
        let input = serde_json::json!({"command": "echo hello"});
        // Should proceed after timeout (not blocked)
        let result = gate.execute("bash", input, test_ctx()).await;
        assert!(result.is_ok());
    }
}
