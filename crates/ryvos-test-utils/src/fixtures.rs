use std::path::PathBuf;

use ryvos_core::config::AppConfig;
use ryvos_core::types::{SessionId, ToolContext};

/// Create a minimal `AppConfig` suitable for testing.
/// Uses sensible defaults and sets model to test-model.
pub fn test_config() -> AppConfig {
    let toml_str = r#"
[model]
provider = "openai"
model_id = "test-model"
api_key = "test-key"

[agent]
max_turns = 5
max_duration_secs = 30
"#;
    toml::from_str(toml_str).expect("test config should parse")
}

/// Create a `ToolContext` for testing with a temporary working directory.
/// The caller should keep the returned `TempDir` alive for the duration
/// of the test to prevent cleanup.
pub fn test_tool_context() -> ToolContext {
    ToolContext {
        session_id: SessionId::from_string("test-session"),
        working_dir: PathBuf::from("/tmp/ryvos-test"),
        store: None,
        agent_spawner: None,
        sandbox_config: None,
        config_path: None,
        viking_client: None,
    }
}

/// Create a `ToolContext` with a specific working directory.
pub fn test_tool_context_with_dir(dir: PathBuf) -> ToolContext {
    ToolContext {
        session_id: SessionId::from_string("test-session"),
        working_dir: dir,
        store: None,
        agent_spawner: None,
        sandbox_config: None,
        config_path: None,
        viking_client: None,
    }
}
