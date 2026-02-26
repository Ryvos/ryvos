use std::sync::Arc;

use futures::StreamExt;
use serde::Deserialize;
use tracing::warn;

use ryvos_core::config::ModelConfig;
use ryvos_core::goal::{CriterionType, Goal};
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{ChatMessage, StreamDelta, Verdict};

/// Two-level Judge system.
///
/// Level 0 (fast): Deterministic checks — OutputContains, OutputEquals.
/// Level 2 (slow): LLM ConversationJudge — evaluates the full conversation
///   against the goal's success criteria and returns a structured Verdict.
pub struct Judge {
    llm: Arc<dyn LlmClient>,
    config: ModelConfig,
}

impl Judge {
    pub fn new(llm: Arc<dyn LlmClient>, config: ModelConfig) -> Self {
        Self { llm, config }
    }

    /// Level 0: Fast deterministic check.
    ///
    /// Returns `Some(Verdict)` if all deterministic criteria can be evaluated
    /// (i.e., the goal has no LlmJudge criteria). Returns `None` if LLM
    /// evaluation is needed.
    pub fn fast_check(output: &str, goal: &Goal) -> Option<Verdict> {
        let has_llm_criteria = goal
            .success_criteria
            .iter()
            .any(|c| matches!(c.criterion_type, CriterionType::LlmJudge { .. }));

        if has_llm_criteria {
            return None; // Need LLM evaluation
        }

        let results = goal.evaluate_deterministic(output);
        let eval = goal.compute_evaluation(results, vec![]);

        if eval.passed {
            Some(Verdict::Accept {
                confidence: eval.overall_score,
            })
        } else {
            let failed: Vec<String> = eval
                .criteria_results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| r.reasoning.clone())
                .collect();

            Some(Verdict::Retry {
                reason: format!(
                    "Score {:.0}% < threshold {:.0}%",
                    eval.overall_score * 100.0,
                    goal.success_threshold * 100.0,
                ),
                hint: if failed.is_empty() {
                    "Try a different approach.".to_string()
                } else {
                    format!("Failed: {}", failed.join("; "))
                },
            })
        }
    }

    /// Level 2: LLM ConversationJudge.
    ///
    /// Sends the full conversation + goal to an LLM and parses a structured verdict.
    pub async fn llm_judge(
        &self,
        conversation: &[ChatMessage],
        goal: &Goal,
    ) -> Result<Verdict, String> {
        let conv_text = conversation
            .iter()
            .filter(|m| m.role != ryvos_core::types::Role::System)
            .map(|m| format!("[{:?}] {}", m.role, m.text()))
            .collect::<Vec<_>>()
            .join("\n");

        let criteria_text = goal
            .success_criteria
            .iter()
            .map(|c| format!("- {} (weight: {})", c.description, c.weight))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            r#"You are a judge evaluating whether an AI agent achieved its goal.

Goal: {}

Success criteria:
{}

Success threshold: {:.0}%

Conversation:
{}

Evaluate the agent's output against the goal and criteria. Respond with ONLY valid JSON:
{{
  "verdict": "accept" | "retry" | "escalate" | "continue",
  "confidence": 0.0-1.0,
  "reason": "brief explanation",
  "hint": "actionable suggestion for retry (only if verdict is retry)"
}}"#,
            goal.description,
            criteria_text,
            goal.success_threshold * 100.0,
            conv_text,
        );

        let messages = vec![ChatMessage::user(prompt)];

        let mut stream = self
            .llm
            .chat_stream(&self.config, messages, &[])
            .await
            .map_err(|e| format!("Judge LLM call failed: {}", e))?;

        let mut response_text = String::new();
        while let Some(delta) = stream.next().await {
            if let Ok(StreamDelta::TextDelta(text)) = delta {
                response_text.push_str(&text);
            }
        }

        parse_verdict(&response_text)
    }

    /// Combined evaluation: try fast check first, fall back to LLM.
    pub async fn evaluate(
        &self,
        output: &str,
        conversation: &[ChatMessage],
        goal: &Goal,
    ) -> Result<Verdict, String> {
        // Level 0: fast check
        if let Some(verdict) = Self::fast_check(output, goal) {
            return Ok(verdict);
        }

        // Level 2: LLM judge
        self.llm_judge(conversation, goal).await
    }
}

/// Response from the LLM judge.
#[derive(Deserialize)]
struct JudgeResponse {
    verdict: String,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    hint: String,
}

/// Parse a Verdict from LLM response text.
fn parse_verdict(response: &str) -> Result<Verdict, String> {
    let json_str = extract_json(response);

    match serde_json::from_str::<JudgeResponse>(json_str) {
        Ok(resp) => match resp.verdict.to_lowercase().as_str() {
            "accept" => Ok(Verdict::Accept {
                confidence: resp.confidence.clamp(0.0, 1.0),
            }),
            "retry" => Ok(Verdict::Retry {
                reason: resp.reason,
                hint: if resp.hint.is_empty() {
                    "Try a different approach.".to_string()
                } else {
                    resp.hint
                },
            }),
            "escalate" => Ok(Verdict::Escalate {
                reason: resp.reason,
            }),
            "continue" => Ok(Verdict::Continue),
            other => {
                warn!(verdict = %other, "Unknown verdict from judge, treating as continue");
                Ok(Verdict::Continue)
            }
        },
        Err(e) => {
            warn!(error = %e, response = %response, "Failed to parse judge response");
            // Default to Continue on parse failure (don't block the agent)
            Ok(Verdict::Continue)
        }
    }
}

/// Extract JSON from a response that may contain markdown code fences.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
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
    use ryvos_core::goal::{CriterionType, SuccessCriterion};

    fn make_goal(criteria: Vec<SuccessCriterion>, threshold: f64) -> Goal {
        Goal {
            description: "test goal".to_string(),
            success_criteria: criteria,
            constraints: vec![],
            success_threshold: threshold,
        }
    }

    fn contains_criterion(id: &str, pattern: &str) -> SuccessCriterion {
        SuccessCriterion {
            id: id.to_string(),
            criterion_type: CriterionType::OutputContains {
                pattern: pattern.to_string(),
                case_sensitive: false,
            },
            weight: 1.0,
            description: format!("contains '{}'", pattern),
        }
    }

    #[test]
    fn test_fast_check_accept() {
        let goal = make_goal(vec![contains_criterion("c1", "hello")], 0.9);
        let verdict = Judge::fast_check("hello world", &goal);
        assert!(verdict.is_some());
        match verdict.unwrap() {
            Verdict::Accept { confidence } => {
                assert!((confidence - 1.0).abs() < 0.01);
            }
            other => panic!("Expected Accept, got {:?}", other),
        }
    }

    #[test]
    fn test_fast_check_retry() {
        let goal = make_goal(vec![contains_criterion("c1", "goodbye")], 0.9);
        let verdict = Judge::fast_check("hello world", &goal);
        assert!(verdict.is_some());
        match verdict.unwrap() {
            Verdict::Retry { reason, hint } => {
                assert!(reason.contains("0%"));
                assert!(hint.contains("goodbye"));
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn test_fast_check_skips_llm_criteria() {
        let criteria = vec![
            contains_criterion("c1", "hello"),
            SuccessCriterion {
                id: "c2".to_string(),
                criterion_type: CriterionType::LlmJudge {
                    prompt: "Is it good?".to_string(),
                },
                weight: 1.0,
                description: "llm judge".to_string(),
            },
        ];
        let goal = make_goal(criteria, 0.5);
        // Returns None because LLM evaluation is needed
        assert!(Judge::fast_check("hello", &goal).is_none());
    }

    #[test]
    fn test_parse_verdict_accept() {
        let response = r#"{"verdict": "accept", "confidence": 0.95, "reason": "all good"}"#;
        let verdict = parse_verdict(response).unwrap();
        match verdict {
            Verdict::Accept { confidence } => assert!((confidence - 0.95).abs() < 0.01),
            other => panic!("Expected Accept, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_verdict_retry() {
        let response = r#"{"verdict": "retry", "confidence": 0.3, "reason": "incomplete", "hint": "add more detail"}"#;
        let verdict = parse_verdict(response).unwrap();
        match verdict {
            Verdict::Retry { reason, hint } => {
                assert_eq!(reason, "incomplete");
                assert_eq!(hint, "add more detail");
            }
            other => panic!("Expected Retry, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_verdict_escalate() {
        let response = r#"{"verdict": "escalate", "reason": "cannot complete"}"#;
        let verdict = parse_verdict(response).unwrap();
        match verdict {
            Verdict::Escalate { reason } => assert_eq!(reason, "cannot complete"),
            other => panic!("Expected Escalate, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_verdict_code_fence() {
        let response = "```json\n{\"verdict\": \"accept\", \"confidence\": 0.8}\n```";
        let verdict = parse_verdict(response).unwrap();
        assert!(matches!(verdict, Verdict::Accept { .. }));
    }

    #[test]
    fn test_parse_verdict_invalid_json() {
        let response = "I'm not sure what to say";
        let verdict = parse_verdict(response).unwrap();
        // Should default to Continue
        assert!(matches!(verdict, Verdict::Continue));
    }

    #[test]
    fn test_parse_verdict_unknown_type() {
        let response = r#"{"verdict": "unknown_type", "confidence": 0.5}"#;
        let verdict = parse_verdict(response).unwrap();
        assert!(matches!(verdict, Verdict::Continue));
    }
}
