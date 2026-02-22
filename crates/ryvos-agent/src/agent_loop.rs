use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use ryvos_core::config::AppConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::traits::{LlmClient, SessionStore};
use ryvos_core::types::*;
use ryvos_tools::ToolRegistry;

use crate::context;
use crate::gate::SecurityGate;
use crate::healing::{FailureJournal, FailureRecord, reflexion_hint_with_history};
use crate::intelligence::{
    compact_tool_output, prune_to_budget, reflexion_hint, summarize_and_prune, FailureTracker,
};

/// Accumulator for streaming tool call deltas.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    input_json: String,
}

/// The agent runtime — runs a ReAct loop with streaming.
pub struct AgentRuntime {
    config: AppConfig,
    llm: Arc<dyn LlmClient>,
    tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
    gate: Option<Arc<SecurityGate>>,
    store: Arc<dyn SessionStore>,
    event_bus: Arc<EventBus>,
    cancel: CancellationToken,
    journal: Option<Arc<FailureJournal>>,
}

impl AgentRuntime {
    pub fn new(
        config: AppConfig,
        llm: impl Into<Arc<dyn LlmClient>>,
        tools: Arc<tokio::sync::RwLock<ToolRegistry>>,
        store: Arc<dyn SessionStore>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            config,
            llm: llm.into(),
            tools,
            gate: None,
            store,
            event_bus,
            cancel: CancellationToken::new(),
            journal: None,
        }
    }

    /// Create an AgentRuntime with a SecurityGate intercepting tool calls.
    pub fn new_with_gate(
        config: AppConfig,
        llm: Arc<dyn LlmClient>,
        gate: Arc<SecurityGate>,
        store: Arc<dyn SessionStore>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        let tools = Arc::new(tokio::sync::RwLock::new(ToolRegistry::new())); // unused when gate is present
        Self {
            config,
            llm,
            tools,
            gate: Some(gate),
            store,
            event_bus,
            cancel: CancellationToken::new(),
            journal: None,
        }
    }

    /// Set the failure journal for self-healing pattern tracking.
    pub fn set_journal(&mut self, journal: Arc<FailureJournal>) {
        self.journal = Some(journal);
    }

    /// Get a cancellation token for this runtime.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Get tool definitions (from gate if present, else from registry).
    async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        if let Some(ref gate) = self.gate {
            gate.definitions().await
        } else {
            self.tools.read().await.definitions()
        }
    }

    /// Execute a tool call — through gate if present, else directly.
    async fn execute_tool(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult> {
        if let Some(ref gate) = self.gate {
            gate.execute(name, input, ctx).await
        } else {
            self.tools.read().await.execute(name, input, ctx).await
        }
    }

    /// Run the agent loop for a given session and user message.
    pub async fn run(
        &self,
        session_id: &SessionId,
        user_message: &str,
    ) -> Result<String> {
        let start = Instant::now();
        let max_turns = self.config.agent.max_turns;
        let max_duration = Duration::from_secs(self.config.agent.max_duration_secs);

        self.event_bus.publish(AgentEvent::RunStarted {
            session_id: session_id.clone(),
        });

        // Build context
        let workspace = self.config.workspace_dir();
        let prompt_override = self
            .config
            .agent
            .system_prompt
            .as_deref()
            .map(|spec| context::resolve_system_prompt(spec, &workspace));
        let system_msg =
            context::build_default_context(&workspace, prompt_override.as_deref());

        // Load history
        let mut messages = vec![system_msg];
        let history = self.store.load_history(session_id, 100).await?;
        messages.extend(history);

        // Append user message
        let user_msg = ChatMessage::user(user_message);
        self.store
            .append_messages(session_id, &[user_msg.clone()])
            .await?;
        messages.push(user_msg);

        // Prune context to fit token budget (with summarization if enabled)
        let budget = self.config.agent.max_context_tokens;
        if self.config.agent.enable_summarization {
            let pruned =
                summarize_and_prune(&mut messages, budget, 6, &*self.llm, &self.config.model)
                    .await?;
            if pruned > 0 {
                info!(pruned, "Summarized and pruned messages to fit context budget");
            }
        } else {
            let pruned = prune_to_budget(&mut messages, budget, 6);
            if pruned > 0 {
                info!(pruned, "Pruned messages to fit context budget");
            }
        }

        let tool_defs = self.tool_definitions().await;
        let max_output_tokens = self.config.agent.max_tool_output_tokens;
        let tool_ctx = ToolContext {
            session_id: session_id.clone(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| workspace.clone()),
            store: Some(self.store.clone()),
            agent_spawner: None, // Set externally when AgentRuntime is wrapped in Arc
            sandbox_config: self.config.agent.sandbox.clone(),
        };

        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;
        #[allow(unused_assignments)]
        let mut final_text = String::new();
        let mut failure_tracker = FailureTracker::default();

        for turn in 0..max_turns {
            // Check cancellation
            if self.cancel.is_cancelled() {
                return Err(RyvosError::Cancelled);
            }

            // Check timeout
            if start.elapsed() > max_duration {
                return Err(RyvosError::MaxDurationExceeded(
                    self.config.agent.max_duration_secs,
                ));
            }

            debug!(turn, "Starting agent turn");

            // Stream from LLM
            let stream_result = tokio::select! {
                result = self.llm.chat_stream(&self.config.model, messages.clone(), &tool_defs) => result,
                _ = self.cancel.cancelled() => return Err(RyvosError::Cancelled),
            };

            let mut stream = stream_result?;

            // Accumulate response
            let mut text_content = String::new();
            let mut thinking_content = String::new();
            let mut tool_calls: Vec<ToolCallAccumulator> = Vec::new();
            let mut stop_reason = None;

            while let Some(delta) = stream.next().await {
                if self.cancel.is_cancelled() {
                    return Err(RyvosError::Cancelled);
                }

                match delta? {
                    StreamDelta::TextDelta(text) => {
                        self.event_bus
                            .publish(AgentEvent::TextDelta(text.clone()));
                        text_content.push_str(&text);
                    }
                    StreamDelta::ThinkingDelta(text) => {
                        thinking_content.push_str(&text);
                    }
                    StreamDelta::ToolUseStart { index, id, name } => {
                        // Ensure vec is large enough
                        while tool_calls.len() <= index {
                            tool_calls.push(ToolCallAccumulator::default());
                        }
                        tool_calls[index].id = id;
                        tool_calls[index].name = name;
                    }
                    StreamDelta::ToolInputDelta { index, delta } => {
                        if let Some(tc) = tool_calls.get_mut(index) {
                            tc.input_json.push_str(&delta);
                        }
                    }
                    StreamDelta::Stop(reason) => {
                        stop_reason = Some(reason);
                    }
                    StreamDelta::Usage {
                        input_tokens,
                        output_tokens,
                    } => {
                        total_input_tokens += input_tokens;
                        total_output_tokens += output_tokens;
                    }
                    StreamDelta::MessageId(_) => {}
                }
            }

            // Build the assistant message
            let mut content_blocks = Vec::new();
            if !thinking_content.is_empty() {
                content_blocks.push(ContentBlock::Thinking {
                    thinking: thinking_content,
                });
            }
            if !text_content.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text_content.clone(),
                });
            }
            for tc in &tool_calls {
                let input: serde_json::Value =
                    serde_json::from_str(&tc.input_json).unwrap_or(serde_json::Value::Null);
                content_blocks.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input,
                });
            }

            let assistant_msg = ChatMessage {
                role: Role::Assistant,
                content: content_blocks,
                timestamp: Some(chrono::Utc::now()),
            };

            self.store
                .append_messages(session_id, &[assistant_msg.clone()])
                .await?;
            messages.push(assistant_msg);

            self.event_bus.publish(AgentEvent::TurnComplete { turn });

            // Check stop reason
            match stop_reason {
                Some(StopReason::EndTurn) | Some(StopReason::StopSequence) | None => {
                    if tool_calls.is_empty() {
                        // No tool calls, we're done
                        final_text = text_content;
                        info!(
                            turn = turn + 1,
                            input_tokens = total_input_tokens,
                            output_tokens = total_output_tokens,
                            "Agent run complete"
                        );
                        self.event_bus.publish(AgentEvent::RunComplete {
                            session_id: session_id.clone(),
                            total_turns: turn + 1,
                            input_tokens: total_input_tokens,
                            output_tokens: total_output_tokens,
                        });
                        return Ok(final_text);
                    }
                }
                Some(StopReason::MaxTokens) => {
                    warn!("LLM hit max tokens");
                    if tool_calls.is_empty() {
                        final_text = text_content;
                        self.event_bus.publish(AgentEvent::RunComplete {
                            session_id: session_id.clone(),
                            total_turns: turn + 1,
                            input_tokens: total_input_tokens,
                            output_tokens: total_output_tokens,
                        });
                        return Ok(final_text);
                    }
                }
                Some(StopReason::ToolUse) => {
                    // Expected, execute tools below
                }
            }

            // Execute tool calls
            // Publish all ToolStart events first (preserves ordering for TUI/gateway)
            let parsed_inputs: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::from_str(&tc.input_json).unwrap_or(serde_json::Value::Null)
                })
                .collect();

            for (tc, input) in tool_calls.iter().zip(parsed_inputs.iter()) {
                self.event_bus.publish(AgentEvent::ToolStart {
                    name: tc.name.clone(),
                    input: input.clone(),
                });
            }

            // Collect (name, id, result) tuples — parallel or serial
            // Note: when gate is present, parallel execution still works because
            // SecurityGate.execute() is &self (shared ref). For approval-requiring
            // tools, each call awaits independently.
            let tool_results: Vec<(String, String, ToolResult)> =
                if self.config.agent.parallel_tools && tool_calls.len() > 1 {
                    // Parallel execution
                    let futs: Vec<_> = tool_calls
                        .iter()
                        .zip(parsed_inputs.into_iter())
                        .map(|(tc, input)| {
                            let gate = self.gate.clone();
                            let tools = Arc::clone(&self.tools);
                            let ctx = tool_ctx.clone();
                            let name = tc.name.clone();
                            let id = tc.id.clone();
                            async move {
                                let result = if let Some(gate) = gate {
                                    gate.execute(&name, input, ctx).await
                                } else {
                                    tools.read().await.execute(&name, input, ctx).await
                                };
                                let tool_result = match result {
                                    Ok(r) => r,
                                    Err(e) => {
                                        error!(tool = %name, error = %e, "Tool execution failed");
                                        ToolResult::error(e.to_string())
                                    }
                                };
                                (name, id, tool_result)
                            }
                        })
                        .collect();
                    futures::future::join_all(futs).await
                } else {
                    // Serial execution
                    let mut results = Vec::with_capacity(tool_calls.len());
                    for (tc, input) in tool_calls.iter().zip(parsed_inputs.into_iter()) {
                        let result = self
                            .execute_tool(&tc.name, input, tool_ctx.clone())
                            .await;
                        let tool_result = match result {
                            Ok(r) => r,
                            Err(e) => {
                                error!(tool = %tc.name, error = %e, "Tool execution failed");
                                ToolResult::error(e.to_string())
                            }
                        };
                        results.push((tc.name.clone(), tc.id.clone(), tool_result));
                    }
                    results
                };

            // Process results: compact output, track failures, build content blocks
            let threshold = self.config.agent.reflexion_failure_threshold;
            let mut tool_result_blocks = Vec::new();

            for (name, id, tool_result) in tool_results {
                let compacted_content =
                    compact_tool_output(&tool_result.content, max_output_tokens);

                let compacted_result = ToolResult {
                    content: compacted_content.clone(),
                    is_error: tool_result.is_error,
                };

                self.event_bus.publish(AgentEvent::ToolEnd {
                    name: name.clone(),
                    result: compacted_result,
                });

                // Track failures and inject reflexion hint when threshold exceeded
                if tool_result.is_error {
                    let count = failure_tracker.record_failure(&name);
                    // Persist to journal
                    if let Some(ref journal) = self.journal {
                        let input_summary = serde_json::to_string(
                            &tool_calls.iter().find(|tc| tc.name == name)
                                .map(|tc| &tc.input_json)
                                .unwrap_or(&String::new()),
                        ).unwrap_or_default();
                        journal.record(FailureRecord {
                            timestamp: chrono::Utc::now(),
                            session_id: session_id.0.clone(),
                            tool_name: name.clone(),
                            error: tool_result.content.clone(),
                            input_summary: input_summary.chars().take(200).collect(),
                            turn,
                        }).ok();
                    }
                    if count >= threshold {
                        // Query past patterns for smarter hint
                        let past = self.journal.as_ref()
                            .and_then(|j| j.find_patterns(&name, 5).ok())
                            .unwrap_or_default();
                        let hint = if past.is_empty() {
                            reflexion_hint(&name, count)
                        } else {
                            reflexion_hint_with_history(&name, count, &past)
                        };
                        messages.push(hint);
                    }
                } else {
                    failure_tracker.record_success(&name);
                    // Record success for health tracking
                    if let Some(ref journal) = self.journal {
                        journal.record_success(&session_id.0, &name).ok();
                    }
                }

                tool_result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: compacted_content,
                    is_error: tool_result.is_error,
                });
            }

            // Add tool results as a user message
            let results_msg = ChatMessage {
                role: Role::User,
                content: tool_result_blocks,
                timestamp: Some(chrono::Utc::now()),
            };

            self.store
                .append_messages(session_id, &[results_msg.clone()])
                .await?;
            messages.push(results_msg);

            // Re-prune before next LLM call (fast, no LLM call mid-loop)
            let pruned = prune_to_budget(&mut messages, budget, 6);
            if pruned > 0 {
                debug!(pruned, "Re-pruned messages after tool execution");
            }

            #[allow(unused_assignments)]
            {
                final_text = text_content;
            }
        }

        Err(RyvosError::MaxTurnsExceeded(max_turns))
    }
}
