pub mod manifest;
pub mod registry;
pub mod skill_tool;

use std::path::Path;

use tracing::{debug, info, warn};

use ryvos_core::traits::Tool;
use ryvos_tools::ToolRegistry;

use manifest::{Prerequisites, SkillManifest};
use skill_tool::SkillTool;

/// Load skills from a directory and register them into the tool registry.
///
/// Scans `dir` for subdirectories containing `skill.toml`, parses each
/// manifest, and registers the corresponding `SkillTool`.
/// Returns the number of skills successfully loaded.
pub fn load_and_register_skills(dir: &Path, registry: &mut ToolRegistry) -> usize {
    let skills = load_skills(dir);
    let count = skills.len();
    for tool in skills {
        info!(name = %tool.name(), "Registered skill");
        registry.register(tool);
    }
    count
}

/// Load skills from a directory, returning a vec of SkillTools.
pub fn load_skills(dir: &Path) -> Vec<SkillTool> {
    let mut tools = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            debug!(path = %dir.display(), error = %e, "Cannot read skills directory");
            return tools;
        }
    };

    for entry in entries.flatten() {
        let skill_dir = entry.path();
        if !skill_dir.is_dir() {
            continue;
        }

        let manifest_path = skill_dir.join("skill.toml");
        if !manifest_path.exists() {
            debug!(path = %skill_dir.display(), "No skill.toml, skipping");
            continue;
        }

        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(path = %manifest_path.display(), error = %e, "Failed to read skill manifest");
                continue;
            }
        };

        let manifest: SkillManifest = match toml::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                warn!(path = %manifest_path.display(), error = %e, "Failed to parse skill manifest");
                continue;
            }
        };

        // Check prerequisites before creating tool
        if let Err(reason) = check_prerequisites(&manifest.prerequisites) {
            warn!(
                skill = %manifest.name,
                reason = %reason,
                "Skipping skill: prerequisites not met"
            );
            continue;
        }

        match SkillTool::new(manifest, skill_dir) {
            Ok(tool) => tools.push(tool),
            Err(e) => {
                warn!(error = %e, "Failed to create skill tool");
            }
        }
    }

    tools
}

/// Check that a skill's prerequisites are met.
/// Returns Ok(()) if all checks pass, or Err with a description of what failed.
fn check_prerequisites(prereqs: &Prerequisites) -> std::result::Result<(), String> {
    // Check required binaries
    for bin in &prereqs.required_binaries {
        if which(bin).is_none() {
            return Err(format!("required binary '{}' not found on PATH", bin));
        }
    }

    // Check required environment variables
    for var in &prereqs.required_env {
        if std::env::var(var).is_err() {
            return Err(format!("required env var '{}' is not set", var));
        }
    }

    // Check required OS
    if let Some(ref required_os) = prereqs.required_os {
        let current_os = std::env::consts::OS;
        let matches = match required_os.as_str() {
            "linux" => current_os == "linux",
            "macos" | "darwin" => current_os == "macos",
            "windows" => current_os == "windows",
            other => {
                return Err(format!("unknown required_os value: '{}'", other));
            }
        };
        if !matches {
            return Err(format!(
                "requires OS '{}', but running on '{}'",
                required_os, current_os
            ));
        }
    }

    Ok(())
}

/// Simple `which` implementation: searches PATH for a binary.
fn which(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(':') {
        let candidate = std::path::Path::new(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_from_temp_dir() {
        let tmp = tempdir();
        let skill_dir = tmp.join("echo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "echo"
description = "Echo back input"
command = "cat"
"#,
        )
        .unwrap();

        let mut registry = ToolRegistry::new();
        let count = load_and_register_skills(&tmp, &mut registry);
        assert_eq!(count, 1);
        assert!(registry.get("echo").is_some());
    }

    #[test]
    fn skip_invalid_manifest() {
        let tmp = tempdir();
        let skill_dir = tmp.join("bad");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("skill.toml"), "not valid toml {{{").unwrap();

        let skills = load_skills(&tmp);
        assert!(skills.is_empty());
    }

    #[test]
    fn skip_missing_dir() {
        let skills = load_skills(Path::new("/nonexistent/path/to/skills"));
        assert!(skills.is_empty());
    }

    #[test]
    fn skip_skill_with_missing_binary() {
        let tmp = tempdir();
        let skill_dir = tmp.join("needs_nonexistent");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "needs_nonexistent"
description = "Needs a missing binary"
command = "echo"

[prerequisites]
required_binaries = ["_ryvos_nonexistent_binary_xyz"]
"#,
        )
        .unwrap();

        let skills = load_skills(&tmp);
        assert!(
            skills.is_empty(),
            "Skill with missing binary should be skipped"
        );
    }

    #[test]
    fn load_skill_with_empty_prerequisites() {
        let tmp = tempdir();
        let skill_dir = tmp.join("no_prereqs");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("skill.toml"),
            r#"
name = "no_prereqs"
description = "No prerequisites"
command = "echo"
"#,
        )
        .unwrap();

        let skills = load_skills(&tmp);
        assert_eq!(skills.len(), 1, "Skill without prerequisites should load");
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ryvos_skills_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
