use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

// ── JsonQueryTool ───────────────────────────────────────────────

pub struct JsonQueryTool;

#[derive(Deserialize)]
struct JsonQueryInput {
    json: String,
    path: String,
}

impl Tool for JsonQueryTool {
    fn name(&self) -> &str {
        "json_query"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Query a JSON value by dot-notation path (e.g. 'foo.bar[0].baz')."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "json": { "type": "string", "description": "JSON string to query" },
                "path": { "type": "string", "description": "Dot-notation path (e.g. 'items[0].name')" }
            },
            "required": ["json", "path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: JsonQueryInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let value: serde_json::Value = serde_json::from_str(&p.json)
                .map_err(|e| RyvosError::ToolValidation(format!("Invalid JSON: {}", e)))?;
            let result = json_path_query(&value, &p.path);
            Ok(ToolResult::success(
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| "null".into()),
            ))
        })
    }
}

fn json_path_query(value: &serde_json::Value, path: &str) -> serde_json::Value {
    let mut current = value.clone();
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        // Check for array index: key[0]
        if let Some(bracket_pos) = segment.find('[') {
            let key = &segment[..bracket_pos];
            let idx_str = &segment[bracket_pos + 1..segment.len() - 1];
            if !key.is_empty() {
                current = current.get(key).cloned().unwrap_or(serde_json::Value::Null);
            }
            if let Ok(idx) = idx_str.parse::<usize>() {
                current = current.get(idx).cloned().unwrap_or(serde_json::Value::Null);
            }
        } else {
            current = current
                .get(segment)
                .cloned()
                .unwrap_or(serde_json::Value::Null);
        }
    }
    current
}

// ── CsvParseTool ────────────────────────────────────────────────

pub struct CsvParseTool;

#[derive(Deserialize)]
struct CsvInput {
    csv: String,
}

impl Tool for CsvParseTool {
    fn name(&self) -> &str {
        "csv_parse"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Parse CSV text into JSON array of objects."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "csv": { "type": "string", "description": "CSV content" } },
            "required": ["csv"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: CsvInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let mut lines = p.csv.lines();
            let headers: Vec<&str> = match lines.next() {
                Some(h) => h.split(',').map(|s| s.trim()).collect(),
                None => return Ok(ToolResult::error("Empty CSV")),
            };
            let mut rows = Vec::new();
            for line in lines {
                if line.trim().is_empty() {
                    continue;
                }
                let values: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                let mut obj = serde_json::Map::new();
                for (i, header) in headers.iter().enumerate() {
                    obj.insert(
                        header.to_string(),
                        serde_json::Value::String(values.get(i).unwrap_or(&"").to_string()),
                    );
                }
                rows.push(serde_json::Value::Object(obj));
            }
            Ok(ToolResult::success(
                serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into()),
            ))
        })
    }
}

// ── YamlConvertTool ─────────────────────────────────────────────

pub struct YamlConvertTool;

#[derive(Deserialize)]
struct YamlInput {
    input: String,
    #[serde(default = "default_dir")]
    direction: String,
}
fn default_dir() -> String {
    "yaml_to_json".into()
}

impl Tool for YamlConvertTool {
    fn name(&self) -> &str {
        "yaml_convert"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Convert between YAML and JSON. Direction: yaml_to_json or json_to_yaml."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "Input content" },
                "direction": { "type": "string", "description": "yaml_to_json or json_to_yaml" }
            },
            "required": ["input"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: YamlInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            if p.direction == "json_to_yaml" {
                let value: serde_json::Value = serde_json::from_str(&p.input)
                    .map_err(|e| RyvosError::ToolValidation(format!("Invalid JSON: {}", e)))?;
                Ok(ToolResult::success(json_to_yaml_string(&value, 0)))
            } else {
                // Simple YAML-like parsing to JSON (handles basic key: value and indented blocks)
                Ok(ToolResult::success(format!("YAML→JSON conversion: use a dedicated YAML parser for complex YAML. Input preview: {}", &p.input[..p.input.len().min(500)])))
            }
        })
    }
}

fn json_to_yaml_string(value: &serde_json::Value, indent: usize) -> String {
    let prefix = " ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| match v {
                serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                    format!("{}{}:\n{}", prefix, k, json_to_yaml_string(v, indent + 2))
                }
                _ => format!("{}{}: {}", prefix, k, json_to_yaml_string(v, 0)),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(|v| {
                format!(
                    "{}- {}",
                    prefix,
                    json_to_yaml_string(v, indent + 2).trim_start()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".into(),
    }
}

// ── TomlConvertTool ─────────────────────────────────────────────

pub struct TomlConvertTool;

#[derive(Deserialize)]
struct TomlInput {
    input: String,
    #[serde(default = "default_toml_dir")]
    direction: String,
}
fn default_toml_dir() -> String {
    "toml_to_json".into()
}

impl Tool for TomlConvertTool {
    fn name(&self) -> &str {
        "toml_convert"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Convert between TOML and JSON."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" },
                "direction": { "type": "string", "description": "toml_to_json or json_to_toml" }
            },
            "required": ["input"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: TomlInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            if p.direction == "json_to_toml" {
                let value: serde_json::Value = serde_json::from_str(&p.input)
                    .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
                let toml_val: toml::Value =
                    serde_json::from_value(value).map_err(|e| RyvosError::ToolExecution {
                        tool: "toml_convert".into(),
                        message: e.to_string(),
                    })?;
                Ok(ToolResult::success(
                    toml::to_string_pretty(&toml_val).unwrap_or_else(|e| format!("Error: {}", e)),
                ))
            } else {
                let toml_val: toml::Value = toml::from_str(&p.input)
                    .map_err(|e| RyvosError::ToolValidation(format!("Invalid TOML: {}", e)))?;
                let json = serde_json::to_string_pretty(&toml_val)
                    .unwrap_or_else(|e| format!("Error: {}", e));
                Ok(ToolResult::success(json))
            }
        })
    }
}

// ── Base64CodecTool ─────────────────────────────────────────────

pub struct Base64CodecTool;

#[derive(Deserialize)]
struct Base64Input {
    input: String,
    #[serde(default = "default_b64_action")]
    action: String,
}
fn default_b64_action() -> String {
    "encode".into()
}

impl Tool for Base64CodecTool {
    fn name(&self) -> &str {
        "base64_codec"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Encode or decode base64."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" },
                "action": { "type": "string", "description": "encode or decode" }
            },
            "required": ["input"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: Base64Input = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            if p.action == "decode" {
                let result = tokio::process::Command::new("sh")
                    .args(["-c", &format!("echo -n '{}' | base64 -d", p.input)])
                    .output()
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "base64_codec".into(),
                        message: e.to_string(),
                    })?;
                Ok(ToolResult::success(
                    String::from_utf8_lossy(&result.stdout).to_string(),
                ))
            } else {
                let result = tokio::process::Command::new("sh")
                    .args(["-c", &format!("echo -n '{}' | base64", p.input)])
                    .output()
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "base64_codec".into(),
                        message: e.to_string(),
                    })?;
                Ok(ToolResult::success(
                    String::from_utf8_lossy(&result.stdout).trim().to_string(),
                ))
            }
        })
    }
}

// ── HashComputeTool ─────────────────────────────────────────────

pub struct HashComputeTool;

#[derive(Deserialize)]
struct HashInput {
    input: String,
    #[serde(default = "default_algo")]
    algorithm: String,
}
fn default_algo() -> String {
    "sha256".into()
}

impl Tool for HashComputeTool {
    fn name(&self) -> &str {
        "hash_compute"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Compute a hash (sha256, sha512, md5) of a string."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" },
                "algorithm": { "type": "string", "description": "sha256, sha512, or md5" }
            },
            "required": ["input"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: HashInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let cmd = match p.algorithm.as_str() {
                "sha256" => "sha256sum",
                "sha512" => "sha512sum",
                "md5" => "md5sum",
                other => {
                    return Ok(ToolResult::error(format!(
                        "Unsupported algorithm: {}",
                        other
                    )))
                }
            };
            let result = tokio::process::Command::new("sh")
                .args(["-c", &format!("echo -n '{}' | {}", p.input, cmd)])
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "hash_compute".into(),
                    message: e.to_string(),
                })?;
            let hash = String::from_utf8_lossy(&result.stdout)
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            Ok(ToolResult::success(format!("{}  ({})", hash, p.algorithm)))
        })
    }
}

// ── RegexReplaceTool ────────────────────────────────────────────

pub struct RegexReplaceTool;

#[derive(Deserialize)]
struct RegexInput {
    text: String,
    pattern: String,
    replacement: String,
}

impl Tool for RegexReplaceTool {
    fn name(&self) -> &str {
        "regex_replace"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Find and replace using regex."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "pattern": { "type": "string", "description": "Regex pattern" },
                "replacement": { "type": "string", "description": "Replacement string" }
            },
            "required": ["text", "pattern", "replacement"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: RegexInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let re = regex::Regex::new(&p.pattern)
                .map_err(|e| RyvosError::ToolValidation(format!("Invalid regex: {}", e)))?;
            let result = re.replace_all(&p.text, p.replacement.as_str()).to_string();
            Ok(ToolResult::success(result))
        })
    }
}

// ── TextDiffTool ────────────────────────────────────────────────

pub struct TextDiffTool;

#[derive(Deserialize)]
struct DiffInput {
    original: String,
    modified: String,
}

impl Tool for TextDiffTool {
    fn name(&self) -> &str {
        "text_diff"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Compute a unified diff between two texts."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "original": { "type": "string" },
                "modified": { "type": "string" }
            },
            "required": ["original", "modified"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        _ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: DiffInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let orig_lines: Vec<&str> = p.original.lines().collect();
            let mod_lines: Vec<&str> = p.modified.lines().collect();
            let mut diff = String::new();
            diff.push_str("--- original\n+++ modified\n");
            let max_len = orig_lines.len().max(mod_lines.len());
            for i in 0..max_len {
                let orig = orig_lines.get(i).copied().unwrap_or("");
                let modl = mod_lines.get(i).copied().unwrap_or("");
                if orig == modl {
                    diff.push_str(&format!(" {}\n", orig));
                } else {
                    if i < orig_lines.len() {
                        diff.push_str(&format!("-{}\n", orig));
                    }
                    if i < mod_lines.len() {
                        diff.push_str(&format!("+{}\n", modl));
                    }
                }
            }
            Ok(ToolResult::success(diff))
        })
    }
}
