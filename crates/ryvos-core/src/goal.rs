use serde::{Deserialize, Serialize};

/// A goal that defines what success looks like for an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    /// Human-readable description of the goal.
    pub description: String,
    /// Weighted criteria that determine success.
    pub success_criteria: Vec<SuccessCriterion>,
    /// Constraints on the execution (time, cost, safety, etc.).
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Minimum weighted score to pass (0.0 to 1.0). Default: 0.9.
    #[serde(default = "default_threshold")]
    pub success_threshold: f64,
}

fn default_threshold() -> f64 {
    0.9
}

/// A single success criterion with a weight and evaluation type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessCriterion {
    /// Unique identifier for this criterion.
    pub id: String,
    /// How to evaluate this criterion.
    pub criterion_type: CriterionType,
    /// Weight for scoring (criteria weights are normalized before scoring).
    #[serde(default = "default_weight")]
    pub weight: f64,
    /// Human-readable description.
    pub description: String,
}

fn default_weight() -> f64 {
    1.0
}

/// The type of evaluation to apply for a criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CriterionType {
    /// Check if the output contains a pattern (fast, no LLM).
    OutputContains {
        pattern: String,
        #[serde(default)]
        case_sensitive: bool,
    },
    /// Check if the output exactly equals an expected value (fast, no LLM).
    OutputEquals { expected: String },
    /// Ask an LLM to judge the output against a custom prompt.
    LlmJudge { prompt: String },
    /// A named custom criterion (evaluated externally).
    Custom { name: String },
}

/// A constraint on the agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Constraint {
    /// Category of constraint.
    pub category: ConstraintCategory,
    /// Whether this is a hard (must not violate) or soft (best effort) constraint.
    pub kind: ConstraintKind,
    /// Human-readable description.
    pub description: String,
    /// Constraint value (interpretation depends on category).
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

/// Category of constraint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintCategory {
    Time,
    Cost,
    Safety,
    Scope,
    Quality,
}

/// Whether a constraint violation is fatal or advisory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    Hard,
    Soft,
}

/// Result of evaluating a single criterion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionResult {
    /// Which criterion was evaluated.
    pub criterion_id: String,
    /// Score for this criterion (0.0 to 1.0).
    pub score: f64,
    /// Whether this criterion passed.
    pub passed: bool,
    /// Explanation of the result.
    pub reasoning: String,
}

/// Result of evaluating a single constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintViolation {
    /// Which constraint was violated.
    pub description: String,
    /// Whether this was a hard or soft constraint.
    pub kind: ConstraintKind,
    /// Details about the violation.
    pub detail: String,
}

/// Overall result of evaluating a goal against output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEvaluation {
    /// Weighted overall score (0.0 to 1.0).
    pub overall_score: f64,
    /// Whether the goal was met (score >= threshold).
    pub passed: bool,
    /// Per-criterion results.
    pub criteria_results: Vec<CriterionResult>,
    /// Constraint violations (empty if none).
    pub constraint_violations: Vec<ConstraintViolation>,
}

impl Goal {
    /// Evaluate the output against all deterministic criteria (OutputContains, OutputEquals).
    /// Returns results for criteria that can be evaluated without an LLM.
    pub fn evaluate_deterministic(&self, output: &str) -> Vec<CriterionResult> {
        self.success_criteria
            .iter()
            .filter_map(|c| match &c.criterion_type {
                CriterionType::OutputContains {
                    pattern,
                    case_sensitive,
                } => {
                    let found = if *case_sensitive {
                        output.contains(pattern)
                    } else {
                        output.to_lowercase().contains(&pattern.to_lowercase())
                    };
                    Some(CriterionResult {
                        criterion_id: c.id.clone(),
                        score: if found { 1.0 } else { 0.0 },
                        passed: found,
                        reasoning: if found {
                            format!("Output contains '{}'", pattern)
                        } else {
                            format!("Output does not contain '{}'", pattern)
                        },
                    })
                }
                CriterionType::OutputEquals { expected } => {
                    let matches = output.trim() == expected.trim();
                    Some(CriterionResult {
                        criterion_id: c.id.clone(),
                        score: if matches { 1.0 } else { 0.0 },
                        passed: matches,
                        reasoning: if matches {
                            "Output matches expected value".to_string()
                        } else {
                            "Output does not match expected value".to_string()
                        },
                    })
                }
                CriterionType::LlmJudge { .. } | CriterionType::Custom { .. } => None,
            })
            .collect()
    }

    /// Compute the overall evaluation from a complete set of criterion results.
    pub fn compute_evaluation(
        &self,
        criteria_results: Vec<CriterionResult>,
        constraint_violations: Vec<ConstraintViolation>,
    ) -> GoalEvaluation {
        let total_weight: f64 = self
            .success_criteria
            .iter()
            .filter(|c| criteria_results.iter().any(|r| r.criterion_id == c.id))
            .map(|c| c.weight)
            .sum();

        let weighted_score = if total_weight > 0.0 {
            criteria_results
                .iter()
                .filter_map(|r| {
                    self.success_criteria
                        .iter()
                        .find(|c| c.id == r.criterion_id)
                        .map(|c| r.score * c.weight)
                })
                .sum::<f64>()
                / total_weight
        } else {
            0.0
        };

        // Hard constraint violations always fail
        let has_hard_violation = constraint_violations
            .iter()
            .any(|v| v.kind == ConstraintKind::Hard);

        let passed = !has_hard_violation && weighted_score >= self.success_threshold;

        GoalEvaluation {
            overall_score: weighted_score,
            passed,
            criteria_results,
            constraint_violations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_goal(criteria: Vec<SuccessCriterion>, threshold: f64) -> Goal {
        Goal {
            description: "test goal".to_string(),
            success_criteria: criteria,
            constraints: vec![],
            success_threshold: threshold,
        }
    }

    fn contains_criterion(id: &str, pattern: &str, weight: f64) -> SuccessCriterion {
        SuccessCriterion {
            id: id.to_string(),
            criterion_type: CriterionType::OutputContains {
                pattern: pattern.to_string(),
                case_sensitive: false,
            },
            weight,
            description: format!("contains '{}'", pattern),
        }
    }

    fn equals_criterion(id: &str, expected: &str, weight: f64) -> SuccessCriterion {
        SuccessCriterion {
            id: id.to_string(),
            criterion_type: CriterionType::OutputEquals {
                expected: expected.to_string(),
            },
            weight,
            description: format!("equals '{}'", expected),
        }
    }

    #[test]
    fn test_goal_evaluation_weighted() {
        let goal = make_goal(
            vec![
                contains_criterion("c1", "hello", 2.0),
                contains_criterion("c2", "world", 1.0),
            ],
            0.6,
        );

        let output = "hello there";
        let results = goal.evaluate_deterministic(output);
        assert_eq!(results.len(), 2);

        let eval = goal.compute_evaluation(results, vec![]);
        // c1 (weight=2): 1.0, c2 (weight=1): 0.0 → (2*1 + 1*0)/3 = 0.667
        assert!((eval.overall_score - 0.667).abs() < 0.01);
        assert!(eval.passed); // 0.667 >= 0.6
    }

    #[test]
    fn test_criterion_types() {
        let goal = make_goal(
            vec![
                contains_criterion("c1", "Hello", 1.0),
                equals_criterion("c2", "Hello World", 1.0),
            ],
            0.9,
        );

        let output = "Hello World";
        let results = goal.evaluate_deterministic(output);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.passed));

        let eval = goal.compute_evaluation(results, vec![]);
        assert!((eval.overall_score - 1.0).abs() < 0.001);
        assert!(eval.passed);
    }

    #[test]
    fn test_constraint_violation() {
        let goal = make_goal(vec![contains_criterion("c1", "ok", 1.0)], 0.5);

        let results = vec![CriterionResult {
            criterion_id: "c1".to_string(),
            score: 1.0,
            passed: true,
            reasoning: "found".to_string(),
        }];

        let violations = vec![ConstraintViolation {
            description: "time limit".to_string(),
            kind: ConstraintKind::Hard,
            detail: "exceeded 60s".to_string(),
        }];

        let eval = goal.compute_evaluation(results, violations);
        assert!(!eval.passed); // hard violation overrides score
        assert_eq!(eval.constraint_violations.len(), 1);
    }

    #[test]
    fn test_case_insensitive_contains() {
        let goal = make_goal(vec![contains_criterion("c1", "SUCCESS", 1.0)], 0.9);

        let output = "The task was a success!";
        let results = goal.evaluate_deterministic(output);
        assert_eq!(results.len(), 1);
        assert!(results[0].passed);
    }

    #[test]
    fn test_empty_criteria() {
        let goal = make_goal(vec![], 0.9);
        let results = goal.evaluate_deterministic("anything");
        assert!(results.is_empty());
        let eval = goal.compute_evaluation(results, vec![]);
        assert!(!eval.passed); // 0.0 < 0.9
    }

    #[test]
    fn test_llm_judge_skipped_in_deterministic() {
        let criteria = vec![
            contains_criterion("c1", "hello", 1.0),
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
        let results = goal.evaluate_deterministic("hello world");
        // Only the deterministic criterion is evaluated
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].criterion_id, "c1");
    }

    #[test]
    fn test_goal_serialization_roundtrip() {
        let goal = Goal {
            description: "Test goal".to_string(),
            success_criteria: vec![
                contains_criterion("c1", "hello", 2.0),
                SuccessCriterion {
                    id: "c2".to_string(),
                    criterion_type: CriterionType::LlmJudge {
                        prompt: "Was it helpful?".to_string(),
                    },
                    weight: 1.0,
                    description: "helpfulness".to_string(),
                },
            ],
            constraints: vec![Constraint {
                category: ConstraintCategory::Time,
                kind: ConstraintKind::Hard,
                description: "Must complete within 60 seconds".to_string(),
                value: Some(serde_json::json!(60)),
            }],
            success_threshold: 0.8,
        };

        let json = serde_json::to_string(&goal).unwrap();
        let parsed: Goal = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.description, "Test goal");
        assert_eq!(parsed.success_criteria.len(), 2);
        assert_eq!(parsed.constraints.len(), 1);
        assert!((parsed.success_threshold - 0.8).abs() < 0.001);
    }
}
