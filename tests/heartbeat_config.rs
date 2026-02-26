use std::io::Write;

use ryvos_core::config::AppConfig;

#[test]
fn test_heartbeat_config_parsing() {
    let toml_content = r#"
[model]
model_id = "llama3.2"

[heartbeat]
enabled = true
interval_secs = 900
target_channel = "telegram"
ack_max_chars = 200
heartbeat_file = "CHECK.md"
prompt = "Custom heartbeat prompt"

[heartbeat.active_hours]
start_hour = 8
end_hour = 20
utc_offset_hours = 2
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    let hb = config.heartbeat.expect("heartbeat present");
    assert!(hb.enabled);
    assert_eq!(hb.interval_secs, 900);
    assert_eq!(hb.target_channel, Some("telegram".to_string()));
    assert_eq!(hb.ack_max_chars, 200);
    assert_eq!(hb.heartbeat_file, "CHECK.md");
    assert_eq!(hb.prompt, Some("Custom heartbeat prompt".to_string()));

    let ah = hb.active_hours.expect("active_hours present");
    assert_eq!(ah.start_hour, 8);
    assert_eq!(ah.end_hour, 20);
    assert_eq!(ah.utc_offset_hours, 2);
}

#[test]
fn test_missing_heartbeat_section_is_none() {
    let toml_content = r#"
[model]
model_id = "llama3.2"
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");
    assert!(config.heartbeat.is_none());
}

#[test]
fn test_heartbeat_defaults() {
    let toml_content = r#"
[model]
model_id = "llama3.2"

[heartbeat]
enabled = true
"#;

    let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
    tmp.write_all(toml_content.as_bytes()).expect("write toml");

    let config = AppConfig::load(tmp.path()).expect("load config");

    let hb = config.heartbeat.expect("heartbeat present");
    assert!(hb.enabled);
    assert_eq!(hb.interval_secs, 1800);
    assert_eq!(hb.target_channel, None);
    assert_eq!(hb.ack_max_chars, 300);
    assert_eq!(hb.heartbeat_file, "HEARTBEAT.md");
    assert_eq!(hb.prompt, None);
    assert!(hb.active_hours.is_none());
}
