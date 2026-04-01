use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Security tier for tool classification.
///
/// **Deprecated:** The tier-based blocking system has been replaced by
/// constitutional self-learning safety. Tiers are retained for backward
/// compatibility with tool trait signatures and config files, but they
/// no longer gate execution. All tools execute freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecurityTier {
    T0,
    T1,
    T2,
    T3,
    T4,
}

impl fmt::Display for SecurityTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::T0 => write!(f, "T0"),
            Self::T1 => write!(f, "T1"),
            Self::T2 => write!(f, "T2"),
            Self::T3 => write!(f, "T3"),
            Self::T4 => write!(f, "T4"),
        }
    }
}

impl std::str::FromStr for SecurityTier {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "t0" => Ok(Self::T0),
            "t1" => Ok(Self::T1),
            "t2" => Ok(Self::T2),
            "t3" => Ok(Self::T3),
            "t4" => Ok(Self::T4),
            other => Err(format!("unknown security tier: {}", other)),
        }
    }
}

/// Security policy — now a passthrough configuration.
///
/// The old blocking/approval logic is removed. This struct is retained
/// for config compatibility. The `pause_before` field replaces the old
/// approval flow with optional soft checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityPolicy {
    /// **Deprecated.** Retained for config compat. No effect.
    #[serde(default = "default_auto_approve")]
    pub auto_approve_up_to: SecurityTier,

    /// **Deprecated.** Retained for config compat. No effect.
    #[serde(default)]
    pub deny_above: Option<SecurityTier>,

    /// Timeout in seconds for soft checkpoint acknowledgment.
    #[serde(default = "default_approval_timeout")]
    pub approval_timeout_secs: u64,

    /// Per-tool tier overrides. Retained for config compat.
    #[serde(default)]
    pub tool_overrides: HashMap<String, SecurityTier>,

    /// **Deprecated.** Regex patterns no longer block tools.
    #[serde(default)]
    pub dangerous_patterns: Vec<DangerousPattern>,

    /// Optional soft checkpoints: tools listed here will pause to explain
    /// reasoning before executing. The agent is NEVER blocked — it just
    /// waits for user acknowledgment. Empty = no pauses.
    #[serde(default)]
    pub pause_before: Vec<String>,
}

fn default_auto_approve() -> SecurityTier {
    SecurityTier::T1
}

fn default_approval_timeout() -> u64 {
    60
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self {
            auto_approve_up_to: SecurityTier::T1,
            deny_above: None, // No denying by default
            approval_timeout_secs: 60,
            tool_overrides: HashMap::new(),
            dangerous_patterns: vec![],
            pause_before: vec![],
        }
    }
}

impl SecurityPolicy {
    /// Kept for backward compat with configs that specify patterns.
    pub fn default_patterns() -> Vec<DangerousPattern> {
        vec![]
    }

    /// Check if a tool should pause for user acknowledgment.
    pub fn should_pause(&self, tool_name: &str) -> bool {
        self.pause_before.iter().any(|t| t == tool_name)
    }
}

/// A pattern that was formerly used to escalate commands to T4.
/// Retained for config backward compatibility. No longer enforced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DangerousPattern {
    pub pattern: String,
    pub label: String,
}

/// A pending approval request — now used only for optional soft checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub tier: SecurityTier,
    pub input_summary: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
}

/// Decision on an approval request.
#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Approved,
    Denied { reason: String },
}

/// **Deprecated.** Retained for backward compat. No longer enforced.
pub struct DangerousPatternMatcher {
    patterns: Vec<(regex::Regex, String)>,
}

impl DangerousPatternMatcher {
    pub fn new(patterns: &[DangerousPattern]) -> Self {
        let compiled = patterns
            .iter()
            .filter_map(|p| match regex::Regex::new(&p.pattern) {
                Ok(re) => Some((re, p.label.clone())),
                Err(e) => {
                    tracing::warn!(
                        pattern = %p.pattern,
                        error = %e,
                        "Invalid dangerous pattern regex, skipping"
                    );
                    None
                }
            })
            .collect();
        Self { patterns: compiled }
    }

    /// Check if a command matches any dangerous pattern. Returns the label if matched.
    /// **Note:** This is informational only. It does NOT block execution.
    pub fn is_dangerous(&self, command: &str) -> Option<&str> {
        for (re, label) in &self.patterns {
            if re.is_match(command) {
                return Some(label.as_str());
            }
        }
        None
    }
}

/// Whether a tool has side effects (used for safety reasoning).
pub fn tool_has_side_effects(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "bash"
            | "write"
            | "edit"
            | "file_delete"
            | "file_move"
            | "file_copy"
            | "dir_create"
            | "git_commit"
            | "git_clone"
            | "git_branch"
            | "http_request"
            | "http_download"
            | "web_fetch"
            | "spawn_agent"
            | "memory_write"
            | "memory_delete"
            | "daily_log_write"
            | "notification_send"
            | "sqlite_query"
            | "archive_create"
            | "archive_extract"
            | "process_kill"
            | "apply_patch"
            | "code_format"
            | "cron_add"
            | "cron_remove"
            | "session_send"
            | "session_spawn"
            | "viking_write"
    )
}

/// Summarize tool input for display in audit logs and soft checkpoints.
pub fn summarize_input(tool_name: &str, input: &serde_json::Value) -> String {
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
                    format!("{}...", s.chars().take(80).collect::<String>())
                } else {
                    s.to_string()
                }
            })
            .unwrap_or_else(|| "<unknown prompt>".to_string()),
        _ => {
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 120 {
                format!("{}...", s.chars().take(120).collect::<String>())
            } else {
                s
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ordering() {
        assert!(SecurityTier::T0 < SecurityTier::T1);
        assert!(SecurityTier::T1 < SecurityTier::T2);
        assert!(SecurityTier::T2 < SecurityTier::T3);
        assert!(SecurityTier::T3 < SecurityTier::T4);
    }

    #[test]
    fn tier_display() {
        assert_eq!(SecurityTier::T0.to_string(), "T0");
        assert_eq!(SecurityTier::T4.to_string(), "T4");
    }

    #[test]
    fn tier_parse() {
        assert_eq!("t0".parse::<SecurityTier>().unwrap(), SecurityTier::T0);
        assert_eq!("T3".parse::<SecurityTier>().unwrap(), SecurityTier::T3);
        assert!("t5".parse::<SecurityTier>().is_err());
    }

    #[test]
    fn default_policy_no_blocking() {
        let policy = SecurityPolicy::default();
        assert_eq!(policy.deny_above, None); // Nothing denied
        assert!(policy.dangerous_patterns.is_empty()); // No patterns
        assert!(policy.pause_before.is_empty()); // No pauses
    }

    #[test]
    fn should_pause() {
        let policy = SecurityPolicy {
            pause_before: vec!["bash".to_string(), "file_delete".to_string()],
            ..Default::default()
        };
        assert!(policy.should_pause("bash"));
        assert!(policy.should_pause("file_delete"));
        assert!(!policy.should_pause("read"));
    }

    #[test]
    fn tool_side_effects() {
        assert!(tool_has_side_effects("bash"));
        assert!(tool_has_side_effects("write"));
        assert!(tool_has_side_effects("file_delete"));
        assert!(!tool_has_side_effects("read"));
        assert!(!tool_has_side_effects("glob"));
        assert!(!tool_has_side_effects("grep"));
    }

    #[test]
    fn tier_serde_roundtrip() {
        let tier = SecurityTier::T2;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"t2\"");
        let parsed: SecurityTier = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, tier);
    }

    #[test]
    fn summarize_input_bash() {
        let input = serde_json::json!({"command": "ls -la"});
        assert_eq!(summarize_input("bash", &input), "ls -la");
    }

    #[test]
    fn summarize_input_write() {
        let input = serde_json::json!({"file_path": "/tmp/test.txt"});
        assert_eq!(summarize_input("write", &input), "/tmp/test.txt");
    }
}
