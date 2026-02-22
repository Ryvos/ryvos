use std::path::Path;
use tracing::debug;

use ryvos_core::types::ChatMessage;

/// Assemble the system prompt from workspace files.
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
                    self.parts.push(format!("# {}\n\n{}", label, content.trim()));
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

    /// Build the final system message.
    pub fn build(self) -> ChatMessage {
        let system_prompt = self.parts.join("\n\n---\n\n");
        ChatMessage {
            role: ryvos_core::types::Role::System,
            content: vec![ryvos_core::types::ContentBlock::Text {
                text: system_prompt,
            }],
            timestamp: Some(chrono::Utc::now()),
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

/// Build the default context for an agent run.
///
/// When `system_prompt_override` is `Some`, appends it via `with_instructions()`
/// after workspace files.
pub fn build_default_context(workspace: &Path, system_prompt_override: Option<&str>) -> ChatMessage {
    let mut builder = ContextBuilder::new()
        .with_base_prompt(DEFAULT_SYSTEM_PROMPT)
        .with_file(&workspace.join("AGENTS.toml"), "Agent Configuration")
        .with_file(&workspace.join("SOUL.md"), "Agent Personality")
        .with_file(&workspace.join("TOOLS.md"), "Tool Usage Conventions")
        .with_file(&workspace.join("USER.md"), "Operator Information")
        .with_file(&workspace.join("IDENTITY.md"), "Agent Identity")
        .with_file(&workspace.join("BOOT.md"), "Boot Instructions")
        .with_file(&workspace.join("HEARTBEAT.md"), "Periodic Status");

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

        let result = resolve_system_prompt(
            &format!("file:{}", file_path.display()),
            Path::new("/tmp"),
        );
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
}
