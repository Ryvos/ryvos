use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Shared context for passing data between graph nodes.
///
/// Each node can read input from and write output to the HandoffContext.
/// Keys are strings; values are JSON for maximum flexibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandoffContext {
    data: HashMap<String, serde_json::Value>,
}

impl HandoffContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a HandoffContext from initial data.
    pub fn from_map(data: HashMap<String, serde_json::Value>) -> Self {
        Self { data }
    }

    /// Get a value by key.
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }

    /// Get a value as a string, if it's a string.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.data.get(key).and_then(|v| v.as_str())
    }

    /// Set a value.
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    /// Set a string value.
    pub fn set_str(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.data
            .insert(key.into(), serde_json::Value::String(value.into()));
    }

    /// Merge another context into this one (overwrites on conflict).
    pub fn merge(&mut self, other: &HandoffContext) {
        for (k, v) in &other.data {
            self.data.insert(k.clone(), v.clone());
        }
    }

    /// Extract output values for the given keys from a node's output.
    ///
    /// For each key, the output text is stored as the value.
    /// If the output contains JSON, individual keys may be extracted.
    pub fn ingest_output(&mut self, output_keys: &[String], output_text: &str) {
        if output_keys.is_empty() {
            return;
        }

        // Try to parse the output as JSON for key extraction
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(output_text) {
            if let Some(obj) = json.as_object() {
                for key in output_keys {
                    if let Some(val) = obj.get(key) {
                        self.data.insert(key.clone(), val.clone());
                    }
                }
                return;
            }
        }

        // Fallback: store the full output under each output key
        for key in output_keys {
            self.data.insert(
                key.clone(),
                serde_json::Value::String(output_text.to_string()),
            );
        }
    }

    /// Get the underlying data map.
    pub fn data(&self) -> &HashMap<String, serde_json::Value> {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut ctx = HandoffContext::new();
        ctx.set_str("name", "Alice");
        ctx.set("count", serde_json::json!(42));

        assert_eq!(ctx.get_str("name"), Some("Alice"));
        assert_eq!(ctx.get("count"), Some(&serde_json::json!(42)));
        assert_eq!(ctx.get("missing"), None);
    }

    #[test]
    fn test_merge() {
        let mut ctx1 = HandoffContext::new();
        ctx1.set_str("a", "1");
        ctx1.set_str("b", "2");

        let mut ctx2 = HandoffContext::new();
        ctx2.set_str("b", "overwritten");
        ctx2.set_str("c", "3");

        ctx1.merge(&ctx2);

        assert_eq!(ctx1.get_str("a"), Some("1"));
        assert_eq!(ctx1.get_str("b"), Some("overwritten"));
        assert_eq!(ctx1.get_str("c"), Some("3"));
    }

    #[test]
    fn test_ingest_json_output() {
        let mut ctx = HandoffContext::new();
        let output = r#"{"findings": "Rust is fast", "score": 9.5}"#;
        ctx.ingest_output(&["findings".into(), "score".into()], output);

        assert_eq!(ctx.get_str("findings"), Some("Rust is fast"));
        assert_eq!(ctx.get("score"), Some(&serde_json::json!(9.5)));
    }

    #[test]
    fn test_ingest_plain_text_output() {
        let mut ctx = HandoffContext::new();
        let output = "This is a plain text result.";
        ctx.ingest_output(&["summary".into()], output);

        assert_eq!(ctx.get_str("summary"), Some("This is a plain text result."));
    }

    #[test]
    fn test_ingest_empty_keys() {
        let mut ctx = HandoffContext::new();
        ctx.ingest_output(&[], "anything");
        assert!(ctx.data().is_empty());
    }

    #[test]
    fn test_from_map() {
        let mut map = HashMap::new();
        map.insert("topic".into(), serde_json::json!("AI"));
        let ctx = HandoffContext::from_map(map);
        assert_eq!(ctx.get_str("topic"), Some("AI"));
    }
}
