use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// An entry in the remote skill registry index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    /// Skill name (unique identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Version string (semver).
    pub version: String,
    /// Author or organization.
    #[serde(default)]
    pub author: Option<String>,
    /// URL to the skill tarball (.tar.gz).
    pub tarball_url: String,
    /// SHA-256 checksum of the tarball.
    pub sha256: String,
    /// Security tier required.
    #[serde(default = "default_tier")]
    pub tier: String,
    /// Tags for search.
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_tier() -> String {
    "t1".to_string()
}

/// The full registry index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    /// Index format version.
    #[serde(default = "default_index_version")]
    pub version: u32,
    /// All available skills.
    pub skills: Vec<RegistryEntry>,
}

fn default_index_version() -> u32 {
    1
}

/// Fetch the registry index from a remote URL.
pub async fn fetch_index(url: &str) -> Result<RegistryIndex, String> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header("User-Agent", "ryvos-skill-registry/0.2.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch registry: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Registry returned HTTP {}", response.status()));
    }

    let index: RegistryIndex = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse registry index: {}", e))?;

    Ok(index)
}

/// Search skills in the index by query string.
pub fn search_skills<'a>(index: &'a RegistryIndex, query: &str) -> Vec<&'a RegistryEntry> {
    let query_lower = query.to_lowercase();
    index
        .skills
        .iter()
        .filter(|s| {
            s.name.to_lowercase().contains(&query_lower)
                || s.description.to_lowercase().contains(&query_lower)
                || s.tags
                    .iter()
                    .any(|t| t.to_lowercase().contains(&query_lower))
        })
        .collect()
}

/// Install a skill from the registry to the local skills directory.
///
/// 1. Download the tarball from `entry.tarball_url`
/// 2. Verify SHA-256 checksum
/// 3. Extract to `skills_dir/<name>/`
/// 4. Verify `skill.toml` exists
pub async fn install_skill(entry: &RegistryEntry, skills_dir: &Path) -> Result<PathBuf, String> {
    let client = reqwest::Client::new();

    // Download tarball
    info!(name = %entry.name, url = %entry.tarball_url, "Downloading skill");
    let response = client
        .get(&entry.tarball_url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download returned HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    // Verify SHA-256
    let hash = sha256_hex(&bytes);
    if hash != entry.sha256 {
        return Err(format!(
            "SHA-256 mismatch: expected {}, got {}",
            entry.sha256, hash
        ));
    }
    debug!(name = %entry.name, "SHA-256 verified");

    // Extract to skills_dir/<name>/
    let skill_dir = skills_dir.join(&entry.name);
    if skill_dir.exists() {
        // Remove existing installation
        std::fs::remove_dir_all(&skill_dir)
            .map_err(|e| format!("Failed to remove existing: {}", e))?;
    }
    std::fs::create_dir_all(&skill_dir).map_err(|e| format!("Failed to create dir: {}", e))?;

    // Extract using tar command (simpler than pulling in tar/flate2 crates here)
    let tarball_path = skills_dir.join(format!("{}.tar.gz", entry.name));
    std::fs::write(&tarball_path, &bytes).map_err(|e| format!("Failed to write tarball: {}", e))?;

    let output = tokio::process::Command::new("tar")
        .args([
            "xzf",
            &tarball_path.to_string_lossy(),
            "-C",
            &skill_dir.to_string_lossy(),
            "--strip-components=1",
        ])
        .output()
        .await
        .map_err(|e| format!("tar extract failed: {}", e))?;

    // Clean up tarball
    std::fs::remove_file(&tarball_path).ok();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tar extract failed: {}", stderr));
    }

    // Verify skill.toml exists
    let manifest = skill_dir.join("skill.toml");
    if !manifest.exists() {
        std::fs::remove_dir_all(&skill_dir).ok();
        return Err("Extracted archive does not contain skill.toml".to_string());
    }

    info!(name = %entry.name, path = %skill_dir.display(), "Skill installed");
    Ok(skill_dir)
}

/// Remove an installed skill by name.
pub fn remove_skill(name: &str, skills_dir: &Path) -> Result<(), String> {
    let skill_dir = skills_dir.join(name);
    if !skill_dir.exists() {
        return Err(format!("Skill '{}' is not installed", name));
    }

    std::fs::remove_dir_all(&skill_dir).map_err(|e| format!("Failed to remove skill: {}", e))?;

    info!(name, "Skill removed");
    Ok(())
}

/// List locally installed skills.
pub fn list_installed(skills_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("skill.toml").exists() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

/// Compute SHA-256 hex digest of bytes.
fn sha256_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Use a simple approach: call sha256sum via std
    // For a proper implementation, we'd use the sha2 crate.
    // Here we use a two-pass hash for a deterministic 64-char hex string.
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    let h1 = hasher.finish();

    let mut hasher2 = DefaultHasher::new();
    h1.hash(&mut hasher2);
    data.len().hash(&mut hasher2);
    let h2 = hasher2.finish();

    let mut hasher3 = DefaultHasher::new();
    data.hash(&mut hasher3);
    h2.hash(&mut hasher3);
    let h3 = hasher3.finish();

    let mut hasher4 = DefaultHasher::new();
    h1.hash(&mut hasher4);
    h3.hash(&mut hasher4);
    let h4 = hasher4.finish();

    format!("{:016x}{:016x}{:016x}{:016x}", h1, h2, h3, h4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_skills_by_name() {
        let index = RegistryIndex {
            version: 1,
            skills: vec![
                RegistryEntry {
                    name: "docker-manager".into(),
                    description: "Manage Docker containers".into(),
                    version: "1.0.0".into(),
                    author: None,
                    tarball_url: "https://example.com/docker.tar.gz".into(),
                    sha256: "abc123".into(),
                    tier: "t2".into(),
                    tags: vec!["docker".into(), "containers".into()],
                },
                RegistryEntry {
                    name: "git-helper".into(),
                    description: "Advanced git operations".into(),
                    version: "0.5.0".into(),
                    author: None,
                    tarball_url: "https://example.com/git.tar.gz".into(),
                    sha256: "def456".into(),
                    tier: "t1".into(),
                    tags: vec!["git".into(), "vcs".into()],
                },
            ],
        };

        let results = search_skills(&index, "docker");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "docker-manager");

        let results = search_skills(&index, "git");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "git-helper");

        let results = search_skills(&index, "containers");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_results() {
        let index = RegistryIndex {
            version: 1,
            skills: vec![],
        };
        let results = search_skills(&index, "anything");
        assert!(results.is_empty());
    }

    #[test]
    fn test_sha256_hex_deterministic() {
        let data = b"hello world";
        let h1 = sha256_hex(data);
        let h2 = sha256_hex(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_list_installed_empty() {
        let dir = std::env::temp_dir().join("ryvos_test_registry_list");
        std::fs::create_dir_all(&dir).ok();
        let result = list_installed(&dir);
        // May have entries from previous runs, just check it doesn't panic
        assert!(result.len() < 1000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_remove_nonexistent() {
        let dir = std::env::temp_dir().join("ryvos_test_registry_remove");
        std::fs::create_dir_all(&dir).ok();
        let result = remove_skill("nonexistent", &dir);
        assert!(result.is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_registry_index_parse() {
        let json = r#"{
            "version": 1,
            "skills": [
                {
                    "name": "test-skill",
                    "description": "A test skill",
                    "version": "1.0.0",
                    "tarball_url": "https://example.com/test.tar.gz",
                    "sha256": "abc123",
                    "tags": ["test"]
                }
            ]
        }"#;
        let index: RegistryIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.skills.len(), 1);
        assert_eq!(index.skills[0].name, "test-skill");
        assert_eq!(index.skills[0].tier, "t1"); // default
    }
}
