use serde::{Deserialize, Serialize};

/// An edge connecting two nodes in the execution graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Source node id.
    pub from: String,
    /// Target node id.
    pub to: String,
    /// Condition that must be true to traverse this edge.
    #[serde(default)]
    pub condition: EdgeCondition,
}

/// Condition for traversing an edge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeCondition {
    /// Always traverse this edge.
    #[default]
    Always,
    /// Traverse only if the source node succeeded.
    OnSuccess,
    /// Traverse only if the source node failed.
    OnFailure,
    /// Traverse if a simple expression matches.
    /// The expression is evaluated against the HandoffContext.
    /// Supported: `key == "value"`, `key != "value"`, `key contains "substr"`.
    Conditional { expr: String },
    /// Ask the LLM to decide whether to traverse.
    LlmDecide { prompt: String },
}

impl Edge {
    /// Create an unconditional edge.
    pub fn always(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: EdgeCondition::Always,
        }
    }

    /// Create an edge that fires on success.
    pub fn on_success(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: EdgeCondition::OnSuccess,
        }
    }

    /// Create an edge that fires on failure.
    pub fn on_failure(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: EdgeCondition::OnFailure,
        }
    }

    /// Create a conditional edge.
    pub fn conditional(
        from: impl Into<String>,
        to: impl Into<String>,
        expr: impl Into<String>,
    ) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            condition: EdgeCondition::Conditional { expr: expr.into() },
        }
    }
}

/// Evaluate a simple conditional expression against context data.
///
/// Supported expressions:
/// - `key == "value"` — exact match
/// - `key != "value"` — not equal
/// - `key contains "substr"` — substring match
///
/// Returns `false` for unparseable expressions.
pub fn evaluate_condition(
    expr: &str,
    context: &std::collections::HashMap<String, serde_json::Value>,
) -> bool {
    let expr = expr.trim();

    // key contains "value"
    if let Some((key, substr)) = parse_operator(expr, "contains") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.contains(substr));
    }

    // key != "value"
    if let Some((key, value)) = parse_operator(expr, "!=") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s != value);
    }

    // key == "value"
    if let Some((key, value)) = parse_operator(expr, "==") {
        return context
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| s == value);
    }

    false
}

/// Parse `key OP "value"` expressions, returning (key, value).
fn parse_operator<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let parts: Vec<&str> = expr.splitn(2, op).collect();
    if parts.len() != 2 {
        return None;
    }
    let key = parts[0].trim();
    let val = parts[1].trim().trim_matches('"');
    Some((key, val))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_edge_builders() {
        let e = Edge::always("a", "b");
        assert_eq!(e.from, "a");
        assert_eq!(e.to, "b");
        assert!(matches!(e.condition, EdgeCondition::Always));

        let e = Edge::on_success("a", "c");
        assert!(matches!(e.condition, EdgeCondition::OnSuccess));

        let e = Edge::on_failure("a", "d");
        assert!(matches!(e.condition, EdgeCondition::OnFailure));
    }

    #[test]
    fn test_condition_equals() {
        let mut ctx = HashMap::new();
        ctx.insert("status".into(), serde_json::json!("success"));

        assert!(evaluate_condition(r#"status == "success""#, &ctx));
        assert!(!evaluate_condition(r#"status == "failure""#, &ctx));
    }

    #[test]
    fn test_condition_not_equals() {
        let mut ctx = HashMap::new();
        ctx.insert("status".into(), serde_json::json!("success"));

        assert!(evaluate_condition(r#"status != "failure""#, &ctx));
        assert!(!evaluate_condition(r#"status != "success""#, &ctx));
    }

    #[test]
    fn test_condition_contains() {
        let mut ctx = HashMap::new();
        ctx.insert(
            "output".into(),
            serde_json::json!("The file was created successfully."),
        );

        assert!(evaluate_condition(r#"output contains "created""#, &ctx));
        assert!(!evaluate_condition(r#"output contains "deleted""#, &ctx));
    }

    #[test]
    fn test_condition_missing_key() {
        let ctx = HashMap::new();
        assert!(!evaluate_condition(r#"missing == "value""#, &ctx));
    }

    #[test]
    fn test_condition_invalid_expr() {
        let ctx = HashMap::new();
        assert!(!evaluate_condition("this is not valid", &ctx));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let edge = Edge {
            from: "a".into(),
            to: "b".into(),
            condition: EdgeCondition::Conditional {
                expr: r#"status == "ok""#.into(),
            },
        };
        let json = serde_json::to_string(&edge).unwrap();
        let parsed: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, "a");
        assert_eq!(parsed.to, "b");
        assert!(matches!(
            parsed.condition,
            EdgeCondition::Conditional { .. }
        ));
    }
}
