use serde::Deserialize;

/// Environment prerequisites for a skill.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Prerequisites {
    /// Required binaries that must be on PATH (e.g., ["python3", "ffmpeg"]).
    #[serde(default)]
    pub required_binaries: Vec<String>,
    /// Required environment variables (e.g., ["OPENAI_API_KEY"]).
    #[serde(default)]
    pub required_env: Vec<String>,
    /// Required OS: "linux", "macos", or "windows".
    #[serde(default)]
    pub required_os: Option<String>,
}

/// TOML manifest for a drop-in skill.
///
/// Lives at `~/.ryvos/skills/<name>/skill.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillManifest {
    /// Unique tool name (e.g., "weather_lookup").
    pub name: String,

    /// Human-readable description shown to the LLM.
    pub description: String,

    /// Shell command to execute. `$SKILL_DIR` is substituted
    /// with the skill's directory path at runtime.
    pub command: String,

    /// Timeout for the command in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Whether the tool requires sandboxed execution.
    #[serde(default)]
    pub requires_sandbox: bool,

    /// JSON string containing the input schema (OpenAI function format).
    #[serde(default = "default_schema")]
    pub input_schema_json: String,

    /// Security tier for this skill (default: "t2").
    #[serde(default = "default_skill_tier")]
    pub tier: String,

    /// Environment prerequisites (optional, backward-compatible).
    #[serde(default)]
    pub prerequisites: Prerequisites,
}

fn default_skill_tier() -> String {
    "t2".to_string()
}

fn default_timeout() -> u64 {
    30
}

fn default_schema() -> String {
    r#"{"type":"object","properties":{}}"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tier() {
        let toml_str = r#"
name = "dangerous_tool"
description = "A dangerous tool"
command = "bash"
tier = "t3"
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.tier, "t3");
    }

    #[test]
    fn parse_tier_defaults_to_t2() {
        let toml_str = r#"
name = "simple"
description = "Simple"
command = "echo"
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.tier, "t2");
    }

    #[test]
    fn parse_full_manifest() {
        let toml_str = r#"
name = "weather_lookup"
description = "Look up current weather for a city"
command = "python3 $SKILL_DIR/weather.py"
timeout_secs = 15
input_schema_json = '{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}'
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "weather_lookup");
        assert_eq!(manifest.timeout_secs, 15);
        assert!(!manifest.requires_sandbox);

        let schema: serde_json::Value = serde_json::from_str(&manifest.input_schema_json).unwrap();
        assert_eq!(schema["properties"]["city"]["type"], "string");
    }

    #[test]
    fn parse_minimal_manifest() {
        let toml_str = r#"
name = "echo"
description = "Echo input"
command = "cat"
"#;
        let manifest: SkillManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.name, "echo");
        assert_eq!(manifest.timeout_secs, 30);
        assert_eq!(
            manifest.input_schema_json,
            r#"{"type":"object","properties":{}}"#
        );
    }
}
