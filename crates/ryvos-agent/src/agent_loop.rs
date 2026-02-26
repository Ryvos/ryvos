use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use ryvos_core::config::AppConfig;
use ryvos_core::error::{Result, RyvosError};
use ryvos_core::event::EventBus;
use ryvos_core::goal::Goal;
use ryvos_core::traits::{LlmClient, SessionStore};
use ryvos_core::types::*;
use ryvos_tools::ToolRegistry;

use crate::checkpoint::CheckpointStore;
use crate::context;
use crate::gate::SecurityGate;
use crate::guardian::GuardianAction;
use crate::healing::{reflexion_hint_with_history, FailureJournal, FailureRecord};
use crate::intelligence::{
    compact_tool_output, is_flush_complete, memory_flush_prompt, prune_to_budget, reflexion_hint,
    summarize_and_prune, FailureTracker,
};
use crate::judge::Judge;
use crate::output_validator::OutputCleaner;

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
    guardian_hints: Option<Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<GuardianAction>>>>,
    checkpoint_store: Option<Arc<CheckpointStore>>,
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
            guardian_hints: None,
            checkpoint_store: None,
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
            guardian_hints: None,
            checkpoint_store: None,
        }
    }

    /// Set the failure journal for self-healing pattern tracking.
    pub fn set_journal(&mut self, journal: Arc<FailureJournal>) {
        self.journal = Some(journal);
    }

    /// Set the Guardian hint receiver for receiving corrective actions.
    pub fn set_guardian_hints(&mut self, rx: tokio::sync::mpsc::Receiver<GuardianAction>) {
        self.guardian_hints = Some(Arc::new(tokio::sync::Mutex::new(rx)));
    }

    /// Set the checkpoint store for save/resume support.
    pub fn set_checkpoint_store(&mut self, store: Arc<CheckpointStore>) {
        self.checkpoint_store = Some(store);
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
    pub async fn run(&self, session_id: &SessionId, user_message: &str) -> Result<String> {
        self.run_with_goal(session_id, user_message, None).await
    }

    /// Run the agent loop with an optional goal.
    /// If a goal is provided, the agent evaluates output against it and retries if not met.
    pub async fn run_with_goal(
        &self,
        session_id: &SessionId,
        user_message: &str,
        goal: Option<&Goal>,
    ) -> Result<String> {
        let start = Instant::now();
        let max_turns = self.config.agent.max_turns;
        let max_duration = Duration::from_secs(self.config.agent.max_duration_secs);

        self.event_bus.publish(AgentEvent::RunStarted {
            session_id: session_id.clone(),
        });

        // Build context (using three-layer onion model)
        let workspace = self.config.workspace_dir();
        let prompt_override = self
            .config
            .agent
            .system_prompt
            .as_deref()
            .map(|spec| context::resolve_system_prompt(spec, &workspace));
        let system_msg = if goal.is_some() {
            context::build_goal_context(&workspace, prompt_override.as_deref(), goal)
        } else {
            context::build_default_context(&workspace, prompt_override.as_deref())
        };

        // Generate a unique run_id for checkpointing
        let run_id = uuid::Uuid::new_v4().to_string();

        // Load history
        let mut messages = vec![system_msg];
        let history = self.store.load_history(session_id, 100).await?;
        messages.extend(history);

        // Append user message
        let user_msg = ChatMessage::user(user_message);
        self.store
            .append_messages(session_id, std::slice::from_ref(&user_msg))
            .await?;
        messages.push(user_msg);

        // Prune context to fit token budget (with summarization if enabled)
        let budget = self.config.agent.max_context_tokens;

        // Memory flush before compaction: if tokens > 85% budget, run a mini-turn
        // to let the agent persist durable info before we prune.
        let flush_disabled = self.config.agent.disable_memory_flush.unwrap_or(false);
        if !flush_disabled {
            let total_tokens: usize = messages
                .iter()
                .map(crate::intelligence::estimate_message_tokens)
                .sum();
            let flush_threshold = (budget as f64 * 0.85) as usize;
            if total_tokens > flush_threshold {
                info!(
                    total_tokens,
                    flush_threshold, "Running memory flush before compaction"
                );
                messages.push(memory_flush_prompt());

                // Run one mini-turn to let agent call memory tools
                let flush_tool_defs = self.tool_definitions().await;
                let flush_ctx = ToolContext {
                    session_id: session_id.clone(),
                    working_dir: std::env::current_dir().unwrap_or_else(|_| workspace.clone()),
                    store: Some(self.store.clone()),
                    agent_spawner: None,
                    sandbox_config: self.config.agent.sandbox.clone(),
                    config_path: None,
                };
                if let Ok(mut stream) = self
                    .llm
                    .chat_stream(&self.config.model, messages.clone(), &flush_tool_defs)
                    .await
                {
                    let mut flush_text = String::new();
                    let mut flush_tool_calls: Vec<ToolCallAccumulator> = Vec::new();
                    while let Some(delta) = stream.next().await {
                        match delta {
                            Ok(StreamDelta::TextDelta(t)) => flush_text.push_str(&t),
                            Ok(StreamDelta::ToolUseStart { index, id, name }) => {
                                while flush_tool_calls.len() <= index {
                                    flush_tool_calls.push(ToolCallAccumulator::default());
                                }
                                flush_tool_calls[index].id = id;
                                flush_tool_calls[index].name = name;
                            }
                            Ok(StreamDelta::ToolInputDelta { index, delta }) => {
                                if let Some(tc) = flush_tool_calls.get_mut(index) {
                                    tc.input_json.push_str(&delta);
                                }
                            }
                            _ => {}
                        }
                    }

                    // Execute any memory tool calls from the flush
                    for tc in &flush_tool_calls {
                        if tc.name.starts_with("memory") || tc.name.starts_with("daily_log") {
                            let input: serde_json::Value =
                                serde_json::from_str(&tc.input_json).unwrap_or_default();
                            let _ = self.execute_tool(&tc.name, input, flush_ctx.clone()).await;
                        }
                    }

                    if is_flush_complete(&flush_text) {
                        debug!("Memory flush completed successfully");
                    }
                }

                // Remove the flush prompt from messages before proceeding
                messages.retain(|m| m.phase() != Some("memory_flush"));
            }
        }

        if self.config.agent.enable_summarization {
            let pruned =
                summarize_and_prune(&mut messages, budget, 6, &*self.llm, &self.config.model)
                    .await?;
            if pruned > 0 {
                info!(
                    pruned,
                    "Summarized and pruned messages to fit context budget"
                );
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
            config_path: None,
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

            // Drain Guardian hints (non-blocking)
            if let Some(ref hints_rx) = self.guardian_hints {
                let mut rx = hints_rx.lock().await;
                while let Ok(action) = rx.try_recv() {
                    match action {
                        GuardianAction::InjectHint(hint) => {
                            debug!(hint = %hint, "Guardian hint injected");
                            messages.push(ChatMessage::user(&hint));
                        }
                        GuardianAction::CancelRun(_) => {
                            return Err(RyvosError::Cancelled);
                        }
                    }
                }
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
                        self.event_bus.publish(AgentEvent::TextDelta(text.clone()));
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
                        self.event_bus.publish(AgentEvent::UsageUpdate {
                            input_tokens,
                            output_tokens,
                        });
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
                metadata: None,
            };

            self.store
                .append_messages(session_id, std::slice::from_ref(&assistant_msg))
                .await?;
            messages.push(assistant_msg);

            self.event_bus.publish(AgentEvent::TurnComplete { turn });

            // Check stop reason
            let is_final_response = tool_calls.is_empty();
            match stop_reason {
                Some(StopReason::EndTurn) | Some(StopReason::StopSequence) | None => {
                    if is_final_response {
                        // Apply heuristic output repair
                        let repaired = OutputCleaner::heuristic_repair(&text_content);
                        final_text = repaired;

                        // Judge evaluation (if goal provided)
                        if let Some(goal) = goal {
                            let judge = Judge::new(self.llm.clone(), self.config.model.clone());
                            match judge.evaluate(&final_text, &messages, goal).await {
                                Ok(verdict) => {
                                    self.event_bus.publish(AgentEvent::JudgeVerdict {
                                        session_id: session_id.clone(),
                                        verdict: verdict.clone(),
                                    });
                                    match &verdict {
                                        Verdict::Accept { confidence } => {
                                            // Also publish GoalEvaluated for backward compat
                                            let results = goal.evaluate_deterministic(&final_text);
                                            let eval = goal.compute_evaluation(results, vec![]);
                                            self.event_bus.publish(AgentEvent::GoalEvaluated {
                                                session_id: session_id.clone(),
                                                evaluation: eval,
                                            });
                                            debug!(confidence, "Judge accepted output");
                                        }
                                        Verdict::Retry { reason, hint } if turn + 1 < max_turns => {
                                            let retry_msg = format!(
                                                "The judge determined your response needs improvement: {}. Hint: {}",
                                                reason, hint
                                            );
                                            messages.push(ChatMessage::user(&retry_msg));
                                            continue;
                                        }
                                        Verdict::Escalate { reason } => {
                                            warn!(reason = %reason, "Judge escalated — returning output as-is");
                                        }
                                        _ => {} // Continue or Retry on last turn
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "Judge evaluation failed, proceeding");
                                }
                            }
                        }

                        // Delete checkpoint on successful completion
                        if let Some(ref cp_store) = self.checkpoint_store {
                            cp_store.delete_run(&session_id.0, &run_id).ok();
                        }

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
                    if is_final_response {
                        final_text = OutputCleaner::heuristic_repair(&text_content);
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

            // Record decisions for tool calls
            let decision_ids: Vec<String> = tool_calls
                .iter()
                .map(|tc| {
                    let decision = Decision {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp: Utc::now(),
                        session_id: session_id.0.clone(),
                        turn,
                        description: format!("Tool call: {}", tc.name),
                        chosen_option: tc.name.clone(),
                        alternatives: if tool_calls.len() > 1 {
                            tool_calls
                                .iter()
                                .filter(|other| other.id != tc.id)
                                .map(|other| DecisionOption {
                                    name: other.name.clone(),
                                    confidence: None,
                                })
                                .collect()
                        } else {
                            vec![]
                        },
                        outcome: None,
                    };
                    if let Some(ref journal) = self.journal {
                        journal.record_decision(&decision).ok();
                    }
                    self.event_bus.publish(AgentEvent::DecisionMade {
                        decision: decision.clone(),
                    });
                    decision.id
                })
                .collect();

            let tool_exec_start = Instant::now();

            // Execute tool calls
            // Publish all ToolStart events first (preserves ordering for TUI/gateway)
            let parsed_inputs: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| serde_json::from_str(&tc.input_json).unwrap_or(serde_json::Value::Null))
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
                        let result = self.execute_tool(&tc.name, input, tool_ctx.clone()).await;
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

            let tool_exec_elapsed_ms = tool_exec_start.elapsed().as_millis() as u64;
            for (idx, (_name, _id, tool_result)) in tool_results.iter().enumerate() {
                // Backfill decision outcome
                if let (Some(ref journal), Some(dec_id)) = (&self.journal, decision_ids.get(idx)) {
                    let outcome = DecisionOutcome {
                        tokens_used: 0, // not tracked per-tool
                        latency_ms: tool_exec_elapsed_ms,
                        succeeded: !tool_result.is_error,
                    };
                    journal.update_decision_outcome(dec_id, &outcome).ok();
                }
            }

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
                            &tool_calls
                                .iter()
                                .find(|tc| tc.name == name)
                                .map(|tc| &tc.input_json)
                                .unwrap_or(&String::new()),
                        )
                        .unwrap_or_default();
                        journal
                            .record(FailureRecord {
                                timestamp: chrono::Utc::now(),
                                session_id: session_id.0.clone(),
                                tool_name: name.clone(),
                                error: tool_result.content.clone(),
                                input_summary: input_summary.chars().take(200).collect(),
                                turn,
                            })
                            .ok();
                    }
                    if count >= threshold {
                        // Query past patterns for smarter hint
                        let past = self
                            .journal
                            .as_ref()
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
                metadata: Some(MessageMetadata {
                    protected: true,
                    ..Default::default()
                }),
            };

            self.store
                .append_messages(session_id, std::slice::from_ref(&results_msg))
                .await?;
            messages.push(results_msg);

            // Re-prune before next LLM call (fast, no LLM call mid-loop)
            let pruned = prune_to_budget(&mut messages, budget, 6);
            if pruned > 0 {
                debug!(pruned, "Re-pruned messages after tool execution");
            }

            // Save checkpoint after each turn
            if let Some(ref cp_store) = self.checkpoint_store {
                if let Ok(json) = CheckpointStore::serialize_messages(&messages) {
                    let cp = crate::checkpoint::Checkpoint {
                        session_id: session_id.0.clone(),
                        run_id: run_id.clone(),
                        turn,
                        messages_json: json,
                        total_input_tokens,
                        total_output_tokens,
                        timestamp: Utc::now(),
                    };
                    if let Err(e) = cp_store.save(&cp) {
                        warn!(error = %e, "Failed to save checkpoint");
                    }
                }
            }

            #[allow(unused_assignments)]
            {
                final_text = text_content;
            }
        }

        Err(RyvosError::MaxTurnsExceeded(max_turns))
    }
}
