use std::sync::Arc;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::goal::{ConstraintViolation, CriterionResult, CriterionType, Goal, GoalEvaluation};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{ChatMessage, StreamDelta};

/// Outcome of a run evaluation (LLM-as-judge).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutcome {
    /// Whether the task was completed successfully.
    pub success: bool,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Brief reasoning for the evaluation.
    pub reasoning: String,
    /// Suggested improvements for future runs.
    #[serde(default)]
    pub suggestions: Vec<String>,
}

/// Evaluates agent run outcomes using LLM-as-judge.
pub struct RunEvaluator {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
}

impl RunEvaluator {
    pub fn new(llm: Arc<dyn LlmClient>, config: ModelConfig) -> Self {
        Self { llm, config }
    }

    /// Evaluate whether an agent run was successful.
    pub async fn evaluate(
        &self,
        user_prompt: &str,
        agent_response: &str,
        tools_used: &str,
    ) -> Result<RunOutcome, String> {
        let eval_prompt = format!(
            r#"You are evaluating whether an AI agent successfully completed a user's task.

User's request:
{}

Agent's final response:
{}

Tools used during the run:
{}

Evaluate whether the task was completed successfully. Respond with ONLY valid JSON in this format:
{{
  "success": true/false,
  "confidence": 0.0-1.0,
  "reasoning": "brief explanation",
  "suggestions": ["improvement 1", "improvement 2"]
}}"#,
            user_prompt, agent_response, tools_used
        );

        debug!("Running self-evaluation");

        let messages = vec![ChatMessage::user(eval_prompt)];

        let stream_result = self
            .llm
            .chat_stream(&self.config, messages, &[])
            .await
            .map_err(|e| format!("Evaluation LLM call failed: {}", e))?;

        let mut stream = stream_result;
        let mut response_text = String::new();

        while let Some(delta) = stream.next().await {
            if let Ok(StreamDelta::TextDelta(text)) = delta {
                response_text.push_str(&text);
            }
        }

        // Try to parse JSON from response (handle markdown code fences)
        let json_str = extract_json(&response_text);

        match serde_json::from_str::<RunOutcome>(json_str) {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                warn!(
                    error = %e,
                    response = %response_text,
                    "Failed to parse evaluation response"
                );
                // Return a fallback outcome
                Ok(RunOutcome {
                    success: true,
                    confidence: 0.5,
                    reasoning: format!("Evaluation parse failed: {}", e),
                    suggestions: vec![],
                })
            }
        }
    }
}

/// Evaluates agent output against a Goal's success criteria.
pub struct GoalEvaluator {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
}

impl GoalEvaluator {
    pub fn new(llm: Arc<dyn LlmClient>, config: ModelConfig) -> Self {
        Self { llm, config }
    }

    /// Evaluate agent output against a goal.
    /// First evaluates deterministic criteria, then uses LLM for LlmJudge criteria.
    pub async fn evaluate(&self, output: &str, goal: &Goal) -> Result<GoalEvaluation, String> {
        let mut results = goal.evaluate_deterministic(output);

        // Evaluate LlmJudge criteria
        for criterion in &goal.success_criteria {
            if let CriterionType::LlmJudge { prompt } = &criterion.criterion_type {
                let result = self
                    .evaluate_llm_criterion(&criterion.id, prompt, output)
                    .await;
                results.push(result);
            }
        }

        // For Custom criteria, mark as passed with 0.5 score (neutral)
        for criterion in &goal.success_criteria {
            if let CriterionType::Custom { name } = &criterion.criterion_type {
                results.push(CriterionResult {
                    criterion_id: criterion.id.clone(),
                    score: 0.5,
                    passed: true,
                    reasoning: format!("Custom criterion '{}' not evaluated (no handler)", name),
                });
            }
        }

        let violations: Vec<ConstraintViolation> = vec![];
        Ok(goal.compute_evaluation(results, violations))
    }

    async fn evaluate_llm_criterion(
        &self,
        criterion_id: &str,
        judge_prompt: &str,
        output: &str,
    ) -> CriterionResult {
        let eval_prompt = format!(
            r#"You are evaluating an AI agent's output against a specific criterion.

Criterion: {}

Agent output:
{}

Score the output from 0.0 to 1.0 for this criterion. Respond with ONLY valid JSON:
{{"score": 0.0-1.0, "reasoning": "brief explanation"}}"#,
            judge_prompt, output
        );

        let messages = vec![ChatMessage::user(eval_prompt)];

        match self.llm.chat_stream(&self.config, messages, &[]).await {
            Ok(mut stream) => {
                let mut response_text = String::new();
                while let Some(delta) = stream.next().await {
                    if let Ok(StreamDelta::TextDelta(text)) = delta {
                        response_text.push_str(&text);
                    }
                }

                let json_str = extract_json(&response_text);

                #[derive(Deserialize)]
                struct JudgeResponse {
                    score: f64,
                    #[serde(default)]
                    reasoning: String,
                }

                match serde_json::from_str::<JudgeResponse>(json_str) {
                    Ok(resp) => CriterionResult {
                        criterion_id: criterion_id.to_string(),
                        score: resp.score.clamp(0.0, 1.0),
                        passed: resp.score >= 0.5,
                        reasoning: resp.reasoning,
                    },
                    Err(e) => {
                        warn!(
                            error = %e,
                            response = %response_text,
                            "Failed to parse LLM judge response"
                        );
                        CriterionResult {
                            criterion_id: criterion_id.to_string(),
                            score: 0.5,
                            passed: true,
                            reasoning: format!("Judge parse failed: {}", e),
                        }
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "LLM judge call failed");
                CriterionResult {
                    criterion_id: criterion_id.to_string(),
                    score: 0.5,
                    passed: true,
                    reasoning: format!("Judge call failed: {}", e),
                }
            }
        }
    }
}

/// Extract JSON from a response that may contain markdown code fences.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    // Try to find JSON in code fence
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim();
        }
    }
    // Try to find JSON object directly
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_plain() {
        let input =
            r#"{"success": true, "confidence": 0.9, "reasoning": "done", "suggestions": []}"#;
        let result = extract_json(input);
        let outcome: RunOutcome = serde_json::from_str(result).unwrap();
        assert!(outcome.success);
    }

    #[test]
    fn test_extract_json_code_fence() {
        let input = r#"Here's my evaluation:
```json
{"success": false, "confidence": 0.3, "reasoning": "incomplete", "suggestions": ["try again"]}
```"#;
        let result = extract_json(input);
        let outcome: RunOutcome = serde_json::from_str(result).unwrap();
        assert!(!outcome.success);
        assert_eq!(outcome.suggestions.len(), 1);
    }

    #[test]
    fn test_extract_json_with_text() {
        let input = r#"The evaluation is: {"success": true, "confidence": 0.8, "reasoning": "ok", "suggestions": []} end"#;
        let result = extract_json(input);
        let outcome: RunOutcome = serde_json::from_str(result).unwrap();
        assert!(outcome.success);
    }

    #[test]
    fn test_run_outcome_defaults() {
        let json = r#"{"success": true, "confidence": 1.0, "reasoning": "perfect"}"#;
        let outcome: RunOutcome = serde_json::from_str(json).unwrap();
        assert!(outcome.suggestions.is_empty());
    }
}
