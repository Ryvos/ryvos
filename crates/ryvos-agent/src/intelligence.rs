use std::collections::HashMap;
use std::sync::OnceLock;

use futures::StreamExt;
use tiktoken_rs::CoreBPE;

use ryvos_core::config::ModelConfig;
use ryvos_core::error::Result;
use ryvos_core::traits::LlmClient;
use ryvos_core::types::{ChatMessage, ContentBlock, Role, StreamDelta};

/// Get or initialize the tokenizer for cl100k_base (works for Claude and GPT-4).
fn tokenizer() -> &'static CoreBPE {
    static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();
    TOKENIZER.get_or_init(|| {
        tiktoken_rs::cl100k_base().expect("Failed to load cl100k_base tokenizer")
    })
}

/// Accurate token count using BPE tokenization (cl100k_base).
pub fn estimate_tokens(text: &str) -> usize {
    tokenizer().encode_ordinary(text).len()
}

/// Estimate token count for an entire ChatMessage.
/// Serializes content blocks to JSON and adds 4 tokens overhead per message.
pub fn estimate_message_tokens(msg: &ChatMessage) -> usize {
    let content_str = serde_json::to_string(&msg.content).unwrap_or_default();
    estimate_tokens(&content_str) + 4
}

/// Remove oldest non-system, non-protected messages from the middle until
/// total tokens fit within `budget`. Never removes index 0 (system) or the
/// last `min_tail` messages. Protected messages (metadata.protected == true)
/// are never removed. Returns the number of messages removed.
pub fn prune_to_budget(messages: &mut Vec<ChatMessage>, budget: usize, min_tail: usize) -> usize {
    let mut removed = 0;

    loop {
        let total: usize = messages.iter().map(estimate_message_tokens).sum();
        if total <= budget {
            break;
        }

        let len = messages.len();
        if len <= 1 + min_tail {
            break;
        }

        let tail_start = len - min_tail;

        // Find the first removable message: not system (idx 0), not in tail, not protected
        let remove_idx = (1..tail_start).find(|&idx| !messages[idx].is_protected());

        match remove_idx {
            Some(idx) => {
                messages.remove(idx);
                removed += 1;
            }
            None => break, // All remaining messages are protected
        }
    }

    removed
}

/// Summarize old messages before pruning to preserve context.
///
/// Phase-aware: groups messages by their phase tag before summarizing.
/// Protected messages are kept as-is. Messages within a phase are never
/// split — the entire phase group is summarized together.
pub async fn summarize_and_prune(
    messages: &mut Vec<ChatMessage>,
    budget: usize,
    min_tail: usize,
    llm: &dyn LlmClient,
    config: &ModelConfig,
) -> Result<usize> {
    let total: usize = messages.iter().map(estimate_message_tokens).sum();
    if total <= budget {
        return Ok(0);
    }

    let len = messages.len();
    if len <= 1 + min_tail {
        return Ok(0);
    }

    let summarize_end = len - min_tail;

    // Collect non-protected messages from the summarizable range, grouped by phase.
    // Protected messages are kept as-is.
    let to_summarize: Vec<&ChatMessage> = messages[1..summarize_end]
        .iter()
        .filter(|m| !m.is_protected())
        .collect();

    if to_summarize.is_empty() {
        return Ok(prune_to_budget(messages, budget, min_tail));
    }

    // Group by phase for the summarization prompt
    let mut phase_groups: Vec<(Option<&str>, Vec<String>)> = Vec::new();
    for msg in &to_summarize {
        let phase = msg.phase();
        let text = format!("{:?}: {}", msg.role, msg.text());

        if let Some(last) = phase_groups.last_mut() {
            if last.0 == phase {
                last.1.push(text);
                continue;
            }
        }
        phase_groups.push((phase, vec![text]));
    }

    // Build phase-aware summarization prompt
    let mut conversation_text = String::new();
    for (phase, texts) in &phase_groups {
        if let Some(phase_name) = phase {
            conversation_text.push_str(&format!("\n## Phase: {}\n", phase_name));
        }
        for text in texts {
            conversation_text.push_str(text);
            conversation_text.push('\n');
        }
    }

    let summary_msgs = vec![ChatMessage::user(format!(
        "Summarize the following conversation concisely, preserving key facts, \
         decisions, code snippets, and file paths. If phases are marked, \
         preserve the phase structure in your summary. Output only the summary.\n\n{}",
        conversation_text
    ))];

    let stream_result = llm.chat_stream(config, summary_msgs, &[]).await;

    match stream_result {
        Ok(mut stream) => {
            let mut summary_text = String::new();
            while let Some(delta) = stream.next().await {
                if let Ok(StreamDelta::TextDelta(text)) = delta {
                    summary_text.push_str(&text);
                }
            }

            if summary_text.is_empty() {
                return Ok(prune_to_budget(messages, budget, min_tail));
            }

            let summary_msg = ChatMessage {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: format!("[Conversation Summary]\n{}", summary_text),
                }],
                timestamp: Some(chrono::Utc::now()),
                metadata: Some(ryvos_core::types::MessageMetadata {
                    protected: true,
                    ..Default::default()
                }),
            };

            // Remove all non-protected messages from the summarizable range.
            // Keep protected messages in place.
            let mut removed = 0;
            let mut i = 1;
            while i < messages.len() - min_tail.min(messages.len().saturating_sub(1)) {
                if i >= summarize_end - removed {
                    break;
                }
                if !messages[i].is_protected() {
                    messages.remove(i);
                    removed += 1;
                } else {
                    i += 1;
                }
            }

            // Insert summary after system message (index 1)
            messages.insert(1, summary_msg);

            // If still over budget, fall back to pruning
            let remaining = prune_to_budget(messages, budget, min_tail);
            Ok(removed + remaining)
        }
        Err(_) => {
            Ok(prune_to_budget(messages, budget, min_tail))
        }
    }
}

/// Truncate tool output to fit within `max_tokens * 4` characters.
/// Prefers truncating at a newline boundary. Appends `[truncated]` if shortened.
pub fn compact_tool_output(content: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if content.len() <= max_chars {
        return content.to_string();
    }

    let truncated = &content[..max_chars];
    // Try to find the last newline within the truncated region
    if let Some(nl_pos) = truncated.rfind('\n') {
        format!("{}\n[truncated]", &content[..nl_pos])
    } else {
        format!("{}\n[truncated]", truncated)
    }
}

/// Generate a user-role hint message nudging the LLM to try a different approach.
pub fn reflexion_hint(tool_name: &str, failure_count: usize) -> ChatMessage {
    let text = format!(
        "The tool `{}` has failed {} times in a row. \
         Try a different approach or use a different tool to accomplish the task.",
        tool_name, failure_count
    );
    ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text { text }],
        timestamp: Some(chrono::Utc::now()),
        metadata: None,
    }
}

/// Tracks consecutive failures per tool name.
#[derive(Debug, Default)]
pub struct FailureTracker {
    counts: HashMap<String, usize>,
}

impl FailureTracker {
    /// Record a successful execution — resets the failure count for this tool.
    pub fn record_success(&mut self, tool_name: &str) {
        self.counts.remove(tool_name);
    }

    /// Record a failed execution — increments and returns the new failure count.
    pub fn record_failure(&mut self, tool_name: &str) -> usize {
        let count = self.counts.entry(tool_name.to_string()).or_insert(0);
        *count += 1;
        *count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // tiktoken cl100k_base: "hello" should be 1 token
        let tokens = estimate_tokens("hello");
        assert!(tokens >= 1);
    }

    #[test]
    fn test_estimate_tokens_longer() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let tokens = estimate_tokens(text);
        // Should be a reasonable number of tokens (not the crude char/4 estimate)
        assert!(tokens > 0 && tokens < text.len());
    }

    #[test]
    fn test_estimate_message_tokens() {
        let msg = ChatMessage::user("hello world");
        let tokens = estimate_message_tokens(&msg);
        // Should be > 4 (the overhead)
        assert!(tokens > 4);
    }

    #[test]
    fn test_compact_tool_output_short() {
        let content = "short output";
        let result = compact_tool_output(content, 100);
        assert_eq!(result, content);
    }

    #[test]
    fn test_compact_tool_output_truncates() {
        let content = "line1\nline2\nline3\nline4\nline5";
        // max_tokens=2 → max_chars=8, content="line1\nli" → truncate at newline → "line1"
        let result = compact_tool_output(content, 2);
        assert!(result.contains("[truncated]"));
        assert!(result.len() < content.len());
    }

    #[test]
    fn test_prune_to_budget() {
        let system = ChatMessage {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: "system".to_string(),
            }],
            timestamp: None,
            metadata: None,
        };
        let mut messages: Vec<ChatMessage> = vec![system];
        // Add 20 user messages
        for i in 0..20 {
            messages.push(ChatMessage::user(format!("message {}", i)));
        }

        let original_len = messages.len();
        // Use a small budget to force pruning
        let removed = prune_to_budget(&mut messages, 100, 3);
        assert!(removed > 0);
        assert!(messages.len() < original_len);
        // System message should still be first
        assert_eq!(messages[0].role, Role::System);
        // Last 3 should be preserved
        assert!(messages.len() >= 4); // system + at least min_tail
    }

    #[test]
    fn test_prune_respects_protected() {
        let system = ChatMessage {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: "system".to_string(),
            }],
            timestamp: None,
            metadata: None,
        };
        let mut messages: Vec<ChatMessage> = vec![system];

        // Add a protected message
        messages.push(
            ChatMessage::user("important tool result")
                .with_metadata(ryvos_core::types::MessageMetadata {
                    protected: true,
                    ..Default::default()
                }),
        );

        // Add regular messages
        for i in 0..10 {
            messages.push(ChatMessage::user(format!("message {}", i)));
        }

        let original_len = messages.len();
        let removed = prune_to_budget(&mut messages, 100, 3);
        assert!(removed > 0);
        assert!(messages.len() < original_len);
        // System message still first
        assert_eq!(messages[0].role, Role::System);
        // Protected message still present
        assert!(messages.iter().any(|m| m.is_protected()));
    }

    #[test]
    fn test_failure_tracker() {
        let mut tracker = FailureTracker::default();
        assert_eq!(tracker.record_failure("bash"), 1);
        assert_eq!(tracker.record_failure("bash"), 2);
        assert_eq!(tracker.record_failure("bash"), 3);
        tracker.record_success("bash");
        assert_eq!(tracker.record_failure("bash"), 1);
    }

    #[test]
    fn test_reflexion_hint_content() {
        let hint = reflexion_hint("bash", 3);
        assert_eq!(hint.role, Role::User);
        let text = hint.text();
        assert!(text.contains("bash"));
        assert!(text.contains("3"));
        assert!(text.contains("different approach"));
    }
}
