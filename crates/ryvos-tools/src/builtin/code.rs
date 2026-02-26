use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::security::SecurityTier;
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

fn resolve(p: &str, wd: &std::path::Path) -> PathBuf {
    let path = PathBuf::from(p);
    if path.is_absolute() {
        path
    } else {
        wd.join(path)
    }
}

fn detect_lang(path: &str) -> &str {
    if path.ends_with(".rs") {
        "rust"
    } else if path.ends_with(".py") {
        "python"
    } else if path.ends_with(".js")
        || path.ends_with(".ts")
        || path.ends_with(".jsx")
        || path.ends_with(".tsx")
    {
        "javascript"
    } else if path.ends_with(".go") {
        "go"
    } else if path.ends_with(".java") {
        "java"
    } else if path.ends_with(".c") || path.ends_with(".cpp") || path.ends_with(".h") {
        "c"
    } else {
        "unknown"
    }
}

// ── CodeFormatTool ──────────────────────────────────────────────

pub struct CodeFormatTool;

#[derive(Deserialize)]
struct CodeFormatInput {
    path: String,
}

impl Tool for CodeFormatTool {
    fn name(&self) -> &str {
        "code_format"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Format code using the appropriate formatter (rustfmt, black, prettier)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File or directory to format" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: CodeFormatInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            let lang = detect_lang(&p.path);
            let (cmd, args): (&str, Vec<String>) = match lang {
                "rust" => ("rustfmt", vec![path.to_string_lossy().to_string()]),
                "python" => ("black", vec![path.to_string_lossy().to_string()]),
                "javascript" => (
                    "prettier",
                    vec!["--write".into(), path.to_string_lossy().to_string()],
                ),
                "go" => (
                    "gofmt",
                    vec!["-w".into(), path.to_string_lossy().to_string()],
                ),
                _ => {
                    return Ok(ToolResult::error(format!(
                        "No formatter configured for '{}'",
                        lang
                    )))
                }
            };
            let output = tokio::process::Command::new(cmd)
                .args(&args)
                .current_dir(&ctx.working_dir)
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "code_format".into(),
                    message: format!("{} not found: {}", cmd, e),
                })?;
            if output.status.success() {
                Ok(ToolResult::success(format!(
                    "Formatted {} with {}",
                    p.path, cmd
                )))
            } else {
                Ok(ToolResult::error(
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
        })
    }
}

// ── CodeLintTool ────────────────────────────────────────────────

pub struct CodeLintTool;

#[derive(Deserialize)]
struct CodeLintInput {
    path: String,
}

impl Tool for CodeLintTool {
    fn name(&self) -> &str {
        "code_lint"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T1
    }
    fn description(&self) -> &str {
        "Lint code using the appropriate linter (clippy, pylint, eslint)."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File or project to lint" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: CodeLintInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            let lang = detect_lang(&p.path);
            let (cmd, args): (&str, Vec<String>) = match lang {
                "rust" => ("cargo", vec!["clippy".into()]),
                "python" => ("pylint", vec![path.to_string_lossy().to_string()]),
                "javascript" => ("eslint", vec![path.to_string_lossy().to_string()]),
                "go" => ("golint", vec![path.to_string_lossy().to_string()]),
                _ => {
                    return Ok(ToolResult::error(format!(
                        "No linter configured for '{}'",
                        lang
                    )))
                }
            };
            let output = tokio::process::Command::new(cmd)
                .args(&args)
                .current_dir(&ctx.working_dir)
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "code_lint".into(),
                    message: format!("{} not found: {}", cmd, e),
                })?;
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            Ok(ToolResult::success(combined))
        })
    }
}

// ── TestRunTool ─────────────────────────────────────────────────

pub struct TestRunTool;

impl Tool for TestRunTool {
    fn name(&self) -> &str {
        "test_run"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T2
    }
    fn timeout_secs(&self) -> u64 {
        300
    }
    fn description(&self) -> &str {
        "Detect project type and run tests."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object", "properties": {} })
    }
    fn execute(
        &self,
        _input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let wd = &ctx.working_dir;
            let (cmd, args): (&str, &[&str]) = if wd.join("Cargo.toml").exists() {
                ("cargo", &["test"])
            } else if wd.join("package.json").exists() {
                ("npm", &["test"])
            } else if wd.join("pyproject.toml").exists() || wd.join("setup.py").exists() {
                ("python", &["-m", "pytest"])
            } else if wd.join("go.mod").exists() {
                ("go", &["test", "./..."])
            } else if wd.join("Makefile").exists() {
                ("make", &["test"])
            } else {
                return Ok(ToolResult::error("Cannot detect project type. No Cargo.toml, package.json, pyproject.toml, go.mod, or Makefile found."));
            };
            let output = tokio::process::Command::new(cmd)
                .args(args)
                .current_dir(wd)
                .output()
                .await
                .map_err(|e| RyvosError::ToolExecution {
                    tool: "test_run".into(),
                    message: e.to_string(),
                })?;
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            if output.status.success() {
                Ok(ToolResult::success(format!("Tests passed.\n{}", combined)))
            } else {
                Ok(ToolResult::error(format!("Tests failed.\n{}", combined)))
            }
        })
    }
}

// ── CodeOutlineTool ─────────────────────────────────────────────

pub struct CodeOutlineTool;

#[derive(Deserialize)]
struct OutlineInput {
    path: String,
}

impl Tool for CodeOutlineTool {
    fn name(&self) -> &str {
        "code_outline"
    }
    fn tier(&self) -> SecurityTier {
        SecurityTier::T0
    }
    fn description(&self) -> &str {
        "Extract function/struct/class definitions from a file."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "Source file path" } },
            "required": ["path"]
        })
    }
    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let p: OutlineInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;
            let path = resolve(&p.path, &ctx.working_dir);
            let content =
                tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| RyvosError::ToolExecution {
                        tool: "code_outline".into(),
                        message: e.to_string(),
                    })?;

            let patterns = [
                regex::Regex::new(r"(?m)^(?:pub\s+)?(?:async\s+)?fn\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^(?:pub\s+)?struct\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^(?:pub\s+)?enum\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^(?:pub\s+)?trait\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^(?:pub\s+)?impl(?:<[^>]*>)?\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^class\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^def\s+(\w+)").unwrap(),
                regex::Regex::new(r"(?m)^(?:export\s+)?(?:async\s+)?function\s+(\w+)").unwrap(),
            ];

            let mut output = String::new();
            for (i, line) in content.lines().enumerate() {
                for pat in &patterns {
                    if pat.is_match(line) {
                        output.push_str(&format!("{:>5}: {}\n", i + 1, line.trim()));
                        break;
                    }
                }
            }

            if output.is_empty() {
                Ok(ToolResult::success("No definitions found.".to_string()))
            } else {
                Ok(ToolResult::success(output))
            }
        })
    }
}
