use std::sync::Arc;

use futures::StreamExt;
use tracing::{debug, warn};

use ryvos_core::config::ModelConfig;
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{ChatMessage, StreamDelta};

/// Validates agent output against expected structure.
pub struct OutputValidator {
    /// Keys that must be present in JSON output.
    pub required_keys: Vec<String>,
    /// Maximum allowed output length (characters).
    pub max_length: usize,
    /// Optional JSON schema for validation.
    pub schema: Option<serde_json::Value>,
}

/// Result of output validation.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// Output is valid.
    Valid,
    /// Output has issues.
    Invalid { issues: Vec<String> },
}

impl OutputValidator {
    /// Create a validator with defaults (no required keys, 100K max length, no schema).
    pub fn new() -> Self {
        Self {
            required_keys: vec![],
            max_length: 100_000,
            schema: None,
        }
    }

    /// Validate the output and return any issues found.
    pub fn validate(&self, output: &str) -> ValidationResult {
        let mut issues = Vec::new();

        // Check max length
        if output.len() > self.max_length {
            issues.push(format!(
                "Output exceeds max length: {} > {}",
                output.len(),
                self.max_length
            ));
        }

        // If we have required keys, check that output contains valid JSON with those keys
        if !self.required_keys.is_empty() {
            match serde_json::from_str::<serde_json::Value>(output) {
                Ok(val) => {
                    if let Some(obj) = val.as_object() {
                        for key in &self.required_keys {
                            if !obj.contains_key(key) {
                                issues.push(format!("Missing required key: '{}'", key));
                            }
                        }
                    } else {
                        issues.push("Expected JSON object but got non-object".to_string());
                    }
                }
                Err(e) => {
                    issues.push(format!("Output is not valid JSON: {}", e));
                }
            }
        }

        if issues.is_empty() {
            ValidationResult::Valid
        } else {
            ValidationResult::Invalid { issues }
        }
    }
}

impl Default for OutputValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Cleans and repairs malformed output.
pub struct OutputCleaner {
    llm: Option<Arc<dyn LlmClient>>,
    config: Option<ModelConfig>,
}

impl OutputCleaner {
    /// Create a cleaner with LLM repair capability.
    pub fn new(llm: Arc<dyn LlmClient>, config: ModelConfig) -> Self {
        Self {
            llm: Some(llm),
            config: Some(config),
        }
    }

    /// Create a cleaner with heuristic repair only (no LLM calls).
    pub fn heuristic_only() -> Self {
        Self {
            llm: None,
            config: None,
        }
    }

    /// Apply heuristic repairs to output.
    /// - Strips markdown code fences
    /// - Balances JSON braces
    /// - Trims whitespace
    pub fn heuristic_repair(output: &str) -> String {
        let mut result = output.to_string();

        // Strip markdown code fences
        result = strip_code_fences(&result);

        // Trim whitespace
        result = result.trim().to_string();

        // Balance JSON braces if output looks like JSON
        if result.starts_with('{') || result.starts_with('[') {
            result = balance_braces(&result);
        }

        result
    }

    /// Ask the LLM to fix malformed output.
    pub async fn llm_repair(
        &self,
        output: &str,
        issues: &[String],
    ) -> Result<String, String> {
        let llm = self
            .llm
            .as_ref()
            .ok_or_else(|| "No LLM configured for repair".to_string())?;
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| "No model config for repair".to_string())?;

        let issues_text = issues.join("\n- ");
        let prompt = format!(
            r#"The following output has issues that need to be fixed:

Issues:
- {}

Original output:
{}

Fix the output to resolve these issues. Return ONLY the corrected output, nothing else."#,
            issues_text, output
        );

        debug!("Running LLM output repair");

        let messages = vec![ChatMessage::user(prompt)];
        let stream_result = llm
            .chat_stream(config, messages, &[])
            .await
            .map_err(|e| format!("LLM repair call failed: {}", e))?;

        let mut stream = stream_result;
        let mut repaired = String::new();

        while let Some(delta) = stream.next().await {
            if let Ok(StreamDelta::TextDelta(text)) = delta {
                repaired.push_str(&text);
            }
        }

        if repaired.is_empty() {
            warn!("LLM repair returned empty response");
            Ok(output.to_string())
        } else {
            Ok(Self::heuristic_repair(&repaired))
        }
    }
}

/// Strip markdown code fences from text.
fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();

    // Try ```json ... ``` first
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    // Try ``` ... ```
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        // Skip optional language tag on same line
        let content_start = after.find('\n').map_or(0, |p| p + 1);
        let after = &after[content_start..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    trimmed.to_string()
}

/// Balance JSON braces/brackets by appending missing closers.
fn balance_braces(text: &str) -> String {
    let mut brace_depth: i32 = 0;
    let mut bracket_depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for ch in text.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if in_string {
            continue;
        }
        match ch {
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
    }

    let mut result = text.to_string();
    for _ in 0..bracket_depth {
        result.push(']');
    }
    for _ in 0..brace_depth {
        result.push('}');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heuristic_repair_markdown() {
        let input = r#"```json
{"key": "value", "count": 42}
```"#;
        let result = OutputCleaner::heuristic_repair(input);
        assert_eq!(result, r#"{"key": "value", "count": 42}"#);
    }

    #[test]
    fn test_heuristic_repair_markdown_with_lang() {
        let input = "```python\nprint('hello')\n```";
        let result = OutputCleaner::heuristic_repair(input);
        assert_eq!(result, "print('hello')");
    }

    #[test]
    fn test_json_brace_balancing() {
        let input = r#"{"key": "value", "nested": {"inner": true"#;
        let result = OutputCleaner::heuristic_repair(input);
        assert!(result.ends_with("}}"));
        // Should be valid JSON after balancing
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
    }

    #[test]
    fn test_bracket_balancing() {
        let input = r#"[1, 2, [3, 4"#;
        let result = OutputCleaner::heuristic_repair(input);
        assert!(result.ends_with("]]"));
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
    }

    #[test]
    fn test_balanced_json_unchanged() {
        let input = r#"{"key": "value"}"#;
        let result = OutputCleaner::heuristic_repair(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_non_json_unchanged() {
        let input = "Hello, this is plain text output.";
        let result = OutputCleaner::heuristic_repair(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_validator_required_keys() {
        let validator = OutputValidator {
            required_keys: vec!["name".to_string(), "age".to_string()],
            max_length: 10_000,
            schema: None,
        };

        let valid = r#"{"name": "Alice", "age": 30}"#;
        assert!(matches!(validator.validate(valid), ValidationResult::Valid));

        let missing = r#"{"name": "Alice"}"#;
        match validator.validate(missing) {
            ValidationResult::Invalid { issues } => {
                assert_eq!(issues.len(), 1);
                assert!(issues[0].contains("age"));
            }
            _ => panic!("Expected invalid"),
        }
    }

    #[test]
    fn test_validator_max_length() {
        let validator = OutputValidator {
            required_keys: vec![],
            max_length: 10,
            schema: None,
        };

        let short = "hi";
        assert!(matches!(validator.validate(short), ValidationResult::Valid));

        let long = "this is a very long output string";
        match validator.validate(long) {
            ValidationResult::Invalid { issues } => {
                assert!(issues[0].contains("max length"));
            }
            _ => panic!("Expected invalid"),
        }
    }

    #[test]
    fn test_validator_invalid_json() {
        let validator = OutputValidator {
            required_keys: vec!["key".to_string()],
            max_length: 10_000,
            schema: None,
        };

        let invalid = "not json at all";
        match validator.validate(invalid) {
            ValidationResult::Invalid { issues } => {
                assert!(issues[0].contains("not valid JSON"));
            }
            _ => panic!("Expected invalid"),
        }
    }

    #[test]
    fn test_strip_code_fences_nested_backticks() {
        let input = "```json\n{\"code\": \"use `backticks`\"}\n```";
        let result = strip_code_fences(input);
        assert!(result.contains("backticks"));
    }

    #[test]
    fn test_brace_balancing_with_strings() {
        // Braces inside strings should not count
        let input = r#"{"msg": "use { and }", "open": true"#;
        let result = balance_braces(input);
        assert!(result.ends_with('}'));
        assert!(serde_json::from_str::<serde_json::Value>(&result).is_ok());
    }
}
