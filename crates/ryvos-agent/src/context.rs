use std::path::Path;
use tracing::debug;

use ryvos_core::goal::Goal;
use ryvos_core::types::ChatMessage;

/// Assemble the system prompt from workspace files using a three-layer
/// "onion model":
///
/// - **Layer 1 (Identity):** Core personality — SOUL.md + IDENTITY.md.
///   Innermost layer, always present, defines who the agent is.
/// - **Layer 2 (Narrative):** Context & conventions — AGENTS.toml, USER.md,
///   TOOLS.md, BOOT.md, HEARTBEAT.md, conversation summaries.
/// - **Layer 3 (Focus):** Current task — goal description, constraints,
///   custom instructions, MCP resources.
pub struct ContextBuilder {
    parts: Vec<String>,
}

impl ContextBuilder {
    pub fn new() -> Self {
        Self { parts: Vec::new() }
    }

    /// Add a base system prompt.
    pub fn with_base_prompt(mut self, prompt: &str) -> Self {
        self.parts.push(prompt.to_string());
        self
    }

    /// Load and append a file if it exists.
    pub fn with_file(mut self, path: &Path, label: &str) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    debug!(path = %path.display(), label, "Loaded context file");
                    self.parts
                        .push(format!("# {}\n\n{}", label, content.trim()));
                }
                Err(e) => {
                    debug!(path = %path.display(), error = %e, "Failed to read context file");
                }
            }
        }
        self
    }

    /// Add custom instructions.
    pub fn with_instructions(mut self, instructions: &str) -> Self {
        if !instructions.is_empty() {
            self.parts.push(instructions.to_string());
        }
        self
    }

    /// Add MCP resource listing section.
    pub fn with_mcp_resources(mut self, resources: &[(String, String, String)]) -> Self {
        if !resources.is_empty() {
            let mut section = String::from("## Available MCP Resources\n");
            for (server, uri, name) in resources {
                section.push_str(&format!("- server \"{}\": {} ({})\n", server, uri, name));
            }
            section.push_str("\nUse the mcp_read_resource tool to access these.");
            self.parts.push(section);
        }
        self
    }

    // ── Three-Layer Onion Model ──────────────────────────────────────

    /// Layer 1 (Identity): Load SOUL.md and IDENTITY.md.
    pub fn with_identity_layer(self, workspace: &Path) -> Self {
        self.with_file(&workspace.join("SOUL.md"), "Agent Personality")
            .with_file(&workspace.join("IDENTITY.md"), "Agent Identity")
    }

    /// Layer 2 (Narrative): Load context files that describe conventions,
    /// tools, operator info, and boot state.
    pub fn with_narrative_layer(self, workspace: &Path) -> Self {
        self.with_file(&workspace.join("AGENTS.toml"), "Agent Configuration")
            .with_file(&workspace.join("TOOLS.md"), "Tool Usage Conventions")
            .with_file(&workspace.join("USER.md"), "Operator Information")
            .with_file(&workspace.join("BOOT.md"), "Boot Instructions")
            .with_file(&workspace.join("HEARTBEAT.md"), "Periodic Status")
    }

    /// Layer 2b (Narrative): Inject recent daily log entries.
    ///
    /// Reads `~/.ryvos/memory/YYYY-MM-DD.md` for the last `days` days
    /// and injects them into the context narrative.
    pub fn with_daily_logs(mut self, workspace: &Path, days: usize) -> Self {
        let memory_dir = workspace.join("memory");
        if !memory_dir.exists() {
            return self;
        }

        let today = chrono::Utc::now().date_naive();
        let mut log_entries = Vec::new();

        for i in 0..days {
            let date = today - chrono::Duration::days(i as i64);
            let filename = format!("{}.md", date.format("%Y-%m-%d"));
            let path = memory_dir.join(&filename);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if !content.trim().is_empty() {
                        log_entries.push(format!(
                            "## {}\n{}",
                            date.format("%Y-%m-%d"),
                            content.trim()
                        ));
                    }
                }
            }
        }

        if !log_entries.is_empty() {
            log_entries.reverse(); // Oldest first
            self.parts.push(format!(
                "# Recent Daily Logs\n\n{}",
                log_entries.join("\n\n")
            ));
        }

        self
    }

    /// Layer 2b (Narrative): Inject a conversation summary from a prior compaction.
    pub fn with_summary(mut self, summary: &str) -> Self {
        if !summary.is_empty() {
            self.parts.push(format!(
                "# Previous Conversation Summary\n\n{}",
                summary.trim()
            ));
        }
        self
    }

    /// Layer 3 (Focus): Inject the current goal and constraints.
    pub fn with_focus_layer(mut self, goal: Option<&Goal>) -> Self {
        if let Some(goal) = goal {
            let mut section = format!("# Current Goal\n\n{}", goal.description);
            if !goal.constraints.is_empty() {
                section.push_str("\n\n## Constraints\n");
                for c in &goal.constraints {
                    let kind = match c.kind {
                        ryvos_core::goal::ConstraintKind::Hard => "MUST",
                        ryvos_core::goal::ConstraintKind::Soft => "SHOULD",
                    };
                    section.push_str(&format!("- [{}] {}\n", kind, c.description));
                }
            }
            if !goal.success_criteria.is_empty() {
                section.push_str("\n## Success Criteria\n");
                for c in &goal.success_criteria {
                    section.push_str(&format!("- {} (weight: {:.1})\n", c.description, c.weight));
                }
            }
            self.parts.push(section);
        }
        self
    }

    /// Build the final system message.
    pub fn build(self) -> ChatMessage {
        let system_prompt = self.parts.join("\n\n---\n\n");
        ChatMessage {
            role: ryvos_core::types::Role::System,
            content: vec![ryvos_core::types::ContentBlock::Text {
                text: system_prompt,
            }],
            timestamp: Some(chrono::Utc::now()),
            metadata: None,
        }
    }
}

impl Default for ContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are Ryvos, a fast and capable AI coding agent.

You have access to tools that let you interact with the user's system:
- **bash**: Execute shell commands
- **read**: Read file contents
- **write**: Write files (creating directories as needed)
- **edit**: Make precise edits to existing files

## Guidelines
- Read files before editing them
- Use bash for system commands (git, cargo, npm, etc.)
- Be concise in your responses
- Execute tools to gather information rather than guessing
- When given a task, break it down and execute step by step
"#;

/// Resolve a system prompt spec.
///
/// If `spec` starts with `file:`, reads the file (relative to `workspace` or absolute).
/// Falls back to literal string if file is unreadable.
pub fn resolve_system_prompt(spec: &str, workspace: &Path) -> String {
    if let Some(path_str) = spec.strip_prefix("file:") {
        let path = Path::new(path_str);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        };
        match std::fs::read_to_string(&resolved) {
            Ok(content) => content,
            Err(e) => {
                debug!(path = %resolved.display(), error = %e, "Failed to read system prompt file, using spec as literal");
                spec.to_string()
            }
        }
    } else {
        spec.to_string()
    }
}

/// Build the default context for an agent run using the three-layer onion model.
///
/// When `system_prompt_override` is `Some`, appends it via `with_instructions()`
/// in Layer 3 (Focus).
pub fn build_default_context(
    workspace: &Path,
    system_prompt_override: Option<&str>,
) -> ChatMessage {
    let mut builder = ContextBuilder::new()
        .with_base_prompt(DEFAULT_SYSTEM_PROMPT)
        // Layer 1: Identity
        .with_identity_layer(workspace)
        // Layer 2: Narrative
        .with_narrative_layer(workspace)
        // Layer 2b: Daily logs (last 2 days)
        .with_daily_logs(workspace, 2);

    // Layer 3: Focus (instructions only — no goal in default context)
    if let Some(instructions) = system_prompt_override {
        builder = builder.with_instructions(instructions);
    }

    builder.build()
}

/// Build context for a goal-driven agent run.
///
/// Same three-layer structure but Layer 3 includes the goal description,
/// constraints, and success criteria.
pub fn build_goal_context(
    workspace: &Path,
    system_prompt_override: Option<&str>,
    goal: Option<&Goal>,
) -> ChatMessage {
    let mut builder = ContextBuilder::new()
        .with_base_prompt(DEFAULT_SYSTEM_PROMPT)
        // Layer 1: Identity
        .with_identity_layer(workspace)
        // Layer 2: Narrative
        .with_narrative_layer(workspace)
        // Layer 2b: Daily logs (last 2 days)
        .with_daily_logs(workspace, 2);

    // Layer 3: Focus
    builder = builder.with_focus_layer(goal);
    if let Some(instructions) = system_prompt_override {
        builder = builder.with_instructions(instructions);
    }

    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_system_prompt_literal() {
        let result = resolve_system_prompt("You are a pirate.", Path::new("/tmp"));
        assert_eq!(result, "You are a pirate.");
    }

    #[test]
    fn test_resolve_system_prompt_file() {
        let dir = std::env::temp_dir().join("ryvos_test_prompt");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("prompt.txt");
        std::fs::write(&file_path, "Be helpful.").unwrap();

        let result =
            resolve_system_prompt(&format!("file:{}", file_path.display()), Path::new("/tmp"));
        assert_eq!(result, "Be helpful.");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_resolve_system_prompt_missing_file_fallback() {
        let spec = "file:/nonexistent/path/prompt.txt";
        let result = resolve_system_prompt(spec, Path::new("/tmp"));
        // Falls back to the literal spec string
        assert_eq!(result, spec);
    }

    #[test]
    fn test_onion_model_layers() {
        let dir = std::env::temp_dir().join("ryvos_test_onion");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SOUL.md"), "I am helpful.").unwrap();
        std::fs::write(dir.join("IDENTITY.md"), "I am Ryvos.").unwrap();

        let msg = ContextBuilder::new()
            .with_base_prompt("base prompt")
            .with_identity_layer(&dir)
            .with_narrative_layer(&dir)
            .build();

        let text = msg.text();
        assert!(text.contains("base prompt"));
        assert!(text.contains("I am helpful."));
        assert!(text.contains("I am Ryvos."));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_focus_layer_with_goal() {
        use ryvos_core::goal::*;

        let goal = Goal {
            description: "Write a hello world script".to_string(),
            success_criteria: vec![SuccessCriterion {
                id: "c1".to_string(),
                criterion_type: CriterionType::OutputContains {
                    pattern: "hello".to_string(),
                    case_sensitive: false,
                },
                weight: 1.0,
                description: "contains hello".to_string(),
            }],
            constraints: vec![Constraint {
                category: ConstraintCategory::Time,
                kind: ConstraintKind::Hard,
                description: "Complete within 60 seconds".to_string(),
                value: None,
            }],
            success_threshold: 0.9,
        };

        let msg = ContextBuilder::new()
            .with_base_prompt("test")
            .with_focus_layer(Some(&goal))
            .build();

        let text = msg.text();
        assert!(text.contains("Current Goal"));
        assert!(text.contains("hello world script"));
        assert!(text.contains("[MUST]"));
        assert!(text.contains("contains hello"));
    }

    #[test]
    fn test_build_goal_context() {
        let dir = std::env::temp_dir().join("ryvos_test_goal_ctx");
        std::fs::create_dir_all(&dir).unwrap();

        let msg = build_goal_context(&dir, Some("extra instructions"), None);
        let text = msg.text();
        assert!(text.contains("extra instructions"));
        // Without a goal, no "Current Goal" section
        assert!(!text.contains("Current Goal"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
