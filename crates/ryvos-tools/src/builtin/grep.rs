use std::path::PathBuf;

use futures::future::BoxFuture;
use serde::Deserialize;
use tracing::debug;

use ryvos_core::error::{Result, RyvosError};
use ryvos_core::traits::Tool;
use ryvos_core::types::{ToolContext, ToolResult};

pub struct GrepTool;

#[derive(Deserialize)]
struct GrepInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob_filter: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    context_lines: Option<usize>,
}

impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn tier(&self) -> ryvos_core::security::SecurityTier {
        ryvos_core::security::SecurityTier::T0
    }

    fn description(&self) -> &str {
        "Search file contents using regex patterns. Returns matching lines with file paths and line numbers."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for (e.g. \"fn main\", \"TODO.*fix\")"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory to search in (default: working directory)"
                },
                "glob_filter": {
                    "type": "string",
                    "description": "Only search files matching this glob (e.g. \"*.rs\", \"*.py\")"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return (default: 50)"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match (default: 0)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute(
        &self,
        input: serde_json::Value,
        ctx: ToolContext,
    ) -> BoxFuture<'_, Result<ToolResult>> {
        Box::pin(async move {
            let params: GrepInput = serde_json::from_value(input)
                .map_err(|e| RyvosError::ToolValidation(e.to_string()))?;

            let re = regex::Regex::new(&params.pattern)
                .map_err(|e| RyvosError::ToolValidation(format!("Invalid regex: {}", e)))?;

            let base = match &params.path {
                Some(p) => {
                    let path = PathBuf::from(p);
                    if path.is_absolute() {
                        path
                    } else {
                        ctx.working_dir.join(path)
                    }
                }
                None => ctx.working_dir.clone(),
            };

            let max_results = params.max_results.unwrap_or(50);
            let context_lines = params.context_lines.unwrap_or(0);

            debug!(pattern = %params.pattern, path = %base.display(), "Grep search");

            // Compile glob filter if provided
            let glob_pattern = params
                .glob_filter
                .as_deref()
                .map(glob::Pattern::new)
                .transpose()
                .map_err(|e| RyvosError::ToolValidation(format!("Invalid glob filter: {}", e)))?;

            let mut results = Vec::new();
            let mut total_matches = 0usize;

            // Walk directory tree
            if base.is_file() {
                search_file(
                    &base,
                    &re,
                    context_lines,
                    max_results,
                    &mut results,
                    &mut total_matches,
                );
            } else {
                for entry in walkdir::WalkDir::new(&base)
                    .follow_links(true)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    if !entry.file_type().is_file() {
                        continue;
                    }

                    let path = entry.path();

                    // Apply glob filter
                    if let Some(ref pat) = glob_pattern {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if !pat.matches(&name) {
                            continue;
                        }
                    }

                    // Skip binary files (check first 512 bytes)
                    if is_likely_binary(path) {
                        continue;
                    }

                    if results.len() >= max_results {
                        break;
                    }

                    search_file(
                        path,
                        &re,
                        context_lines,
                        max_results,
                        &mut results,
                        &mut total_matches,
                    );
                }
            }

            let output = if results.is_empty() {
                "No matches found.".to_string()
            } else {
                let header = if total_matches > results.len() {
                    format!(
                        "{} matches found (showing first {}):\n",
                        total_matches,
                        results.len()
                    )
                } else {
                    format!("{} matches found:\n", total_matches)
                };
                format!("{}{}", header, results.join("\n"))
            };

            Ok(ToolResult::success(output))
        })
    }
}

fn search_file(
    path: &std::path::Path,
    re: &regex::Regex,
    context_lines: usize,
    max_results: usize,
    results: &mut Vec<String>,
    total_matches: &mut usize,
) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let lines: Vec<&str> = content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            *total_matches += 1;

            if results.len() < max_results {
                if context_lines > 0 {
                    let start = i.saturating_sub(context_lines);
                    let end = (i + context_lines + 1).min(lines.len());
                    for (j, line) in lines.iter().enumerate().take(end).skip(start) {
                        let marker = if j == i { ">" } else { " " };
                        results.push(format!("{}{}:{}:{}", marker, path.display(), j + 1, line));
                    }
                    results.push("--".to_string());
                } else {
                    results.push(format!("{}:{}:{}", path.display(), i + 1, line));
                }
            }
        }
    }
}

fn is_likely_binary(path: &std::path::Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return true,
    };
    use std::io::Read;
    let mut buf = [0u8; 512];
    let mut reader = std::io::BufReader::new(file);
    let n = match reader.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return true,
    };
    buf[..n].contains(&0)
}
