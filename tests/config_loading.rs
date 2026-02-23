use std::io::Write;

use ryvos_core::config::AppConfig;

#[test]
fn test_load_full_config_from_file() {
    let toml_content = r#"
[agent]
max_turns = 10
workspace = "/tmp/ryvos-test"

[model]
provider = "anthropic"
model_id = "claude-sonnet-4-20250514"
api_key = "sk-test-key"
max_tokens = 4096
temperature = 0.5

[gateway]
bind = "0.0.0.0:9999"
token = "test-token"

[[gateway.api_keys]]
name = "ci"
key = "rk_ci_key"
role = "admin"

[channels.telegram]
bot_token = "123:BOT"
allowed_users = [42]
dm_policy = "allowlist"

[channels.slack]
bot_token = "xoxb-test"
app_token = "xapp-test"

[mcp.servers.web-search]
auto_connect = true

[mcp.servers.web-search.transport]
type = "stdio"
command = "npx"
args = ["-y", "@anthropic/web-search-mcp"]

[hooks]
on_start = ["echo starting"]
on_message = []
on_tool_call = []
on_response = []
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    assert_eq!(config.agent.max_turns, 10);
    assert_eq!(config.model.provider, "anthropic");
    assert_eq!(config.model.model_id, "claude-sonnet-4-20250514");
    assert_eq!(config.model.api_key, Some("sk-test-key".to_string()));
    assert_eq!(config.model.max_tokens, 4096);

    let gw = config.gateway.expect("gateway present");
    assert_eq!(gw.bind, "0.0.0.0:9999");
    assert_eq!(gw.api_keys.len(), 1);
    assert_eq!(gw.api_keys[0].name, "ci");

    assert!(config.channels.telegram.is_some());
    assert!(config.channels.slack.is_some());
    assert!(config.mcp.is_some());

    let hooks = config.hooks.expect("hooks present");
    assert_eq!(hooks.on_start, vec!["echo starting"]);
}

#[test]
fn test_env_var_expansion_in_config() {
    std::env::set_var("RYVOS_TEST_API_KEY", "expanded-key-value");

    let toml_content = r#"
[model]
model_id = "test-model"
api_key = "${RYVOS_TEST_API_KEY}"
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");
    assert_eq!(config.model.api_key, Some("expanded-key-value".to_string()));

    std::env::remove_var("RYVOS_TEST_API_KEY");
}

#[test]
fn test_minimal_config_uses_defaults() {
    let toml_content = r#"
[model]
model_id = "llama3.2"
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    assert_eq!(config.agent.max_turns, 25);
    assert_eq!(config.agent.max_duration_secs, 600);
    assert!(config.agent.parallel_tools);
    assert!(config.gateway.is_none());
    assert!(config.channels.telegram.is_none());
    assert!(config.channels.discord.is_none());
    assert!(config.channels.slack.is_none());
    assert!(config.mcp.is_none());
    assert!(config.hooks.is_none());
}

#[test]
fn test_guardian_config_defaults_from_minimal_toml() {
    let toml_content = r#"
[model]
model_id = "llama3.2"
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    // Guardian should get sensible defaults even without [agent.guardian] section
    assert!(config.agent.guardian.enabled);
    assert_eq!(config.agent.guardian.doom_loop_threshold, 3);
    assert_eq!(config.agent.guardian.stall_timeout_secs, 120);
    assert_eq!(config.agent.guardian.token_budget, 0);
    assert_eq!(config.agent.guardian.token_warn_pct, 80);
}

#[test]
fn test_guardian_config_custom_values() {
    let toml_content = r#"
[model]
model_id = "llama3.2"

[agent.guardian]
enabled = false
doom_loop_threshold = 5
stall_timeout_secs = 60
token_budget = 100000
token_warn_pct = 90
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    assert!(!config.agent.guardian.enabled);
    assert_eq!(config.agent.guardian.doom_loop_threshold, 5);
    assert_eq!(config.agent.guardian.stall_timeout_secs, 60);
    assert_eq!(config.agent.guardian.token_budget, 100000);
    assert_eq!(config.agent.guardian.token_warn_pct, 90);
}
