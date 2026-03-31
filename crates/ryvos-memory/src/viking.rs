use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Context loading level — controls detail granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContextLevel {
    /// Summary only (~100 tokens per entry).
    L0,
    /// Key details — structured fields.
    L1,
    /// Full content — complete memory entry.
    L2,
}

impl std::fmt::Display for ContextLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::L0 => write!(f, "L0"),
            Self::L1 => write!(f, "L1"),
            Self::L2 => write!(f, "L2"),
        }
    }
}

/// A result from Viking memory retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikingResult {
    pub path: String,
    pub content: String,
    pub level: ContextLevel,
    pub relevance_score: f64,
    #[serde(default)]
    pub trajectory: Vec<String>,
}

/// Categories for memory organization in Viking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCategory {
    UserProfile,
    UserPreferences,
    UserEntities,
    AgentEvents,
    AgentCases,
    AgentPatterns,
}

impl MemoryCategory {
    /// Get the viking:// directory path for this category.
    pub fn viking_path(&self) -> &str {
        match self {
            Self::UserProfile => "viking://user/profile/",
            Self::UserPreferences => "viking://user/preferences/",
            Self::UserEntities => "viking://user/entities/",
            Self::AgentEvents => "viking://agent/events/",
            Self::AgentCases => "viking://agent/cases/",
            Self::AgentPatterns => "viking://agent/patterns/",
        }
    }
}

/// Metadata attached to a Viking memory entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VikingMeta {
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_session: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Directory listing entry from Viking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikingDirEntry {
    pub path: String,
    pub is_directory: bool,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub child_count: Option<usize>,
}

/// HTTP client for the OpenViking context database service.
pub struct VikingClient {
    base_url: String,
    client: Client,
    user_id: String,
}

impl VikingClient {
    /// Create a new VikingClient.
    pub fn new(base_url: &str, user_id: &str) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
            user_id: user_id.to_string(),
        }
    }

    /// Health check — returns true if Viking is reachable.
    pub async fn health(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Write a memory entry at the given path.
    pub async fn write_memory(
        &self,
        path: &str,
        content: &str,
        meta: Option<VikingMeta>,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "user_id": self.user_id,
            "path": path,
            "content": content,
            "metadata": meta.unwrap_or_default(),
        });

        let resp = self
            .client
            .post(format!("{}/api/memory/write", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Viking write failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Viking write returned {}", resp.status()))
        }
    }

    /// Read a memory entry at the given path with specified detail level.
    pub async fn read_memory(
        &self,
        path: &str,
        level: ContextLevel,
    ) -> Result<VikingResult, String> {
        let resp = self
            .client
            .get(format!("{}/api/memory/read", self.base_url))
            .query(&[
                ("user_id", self.user_id.as_str()),
                ("path", path),
                ("level", &level.to_string()),
            ])
            .send()
            .await
            .map_err(|e| format!("Viking read failed: {}", e))?;

        if resp.status().is_success() {
            resp.json::<VikingResult>()
                .await
                .map_err(|e| format!("Viking read parse failed: {}", e))
        } else {
            Err(format!("Viking read returned {}", resp.status()))
        }
    }

    /// Search Viking memory with a natural language query.
    pub async fn search(
        &self,
        query: &str,
        directory: Option<&str>,
        limit: usize,
    ) -> Result<Vec<VikingResult>, String> {
        let mut params: Vec<(&str, String)> = vec![
            ("user_id", self.user_id.clone()),
            ("query", query.to_string()),
            ("limit", limit.to_string()),
        ];
        if let Some(dir) = directory {
            params.push(("directory", dir.to_string()));
        }

        let resp = self
            .client
            .get(format!("{}/api/memory/search", self.base_url))
            .query(&params)
            .send()
            .await
            .map_err(|e| format!("Viking search failed: {}", e))?;

        if resp.status().is_success() {
            resp.json::<Vec<VikingResult>>()
                .await
                .map_err(|e| format!("Viking search parse failed: {}", e))
        } else {
            Err(format!("Viking search returned {}", resp.status()))
        }
    }

    /// List directory contents with L0 summaries.
    pub async fn list_directory(&self, path: &str) -> Result<Vec<VikingDirEntry>, String> {
        let resp = self
            .client
            .get(format!("{}/api/memory/list", self.base_url))
            .query(&[("user_id", self.user_id.as_str()), ("path", path)])
            .send()
            .await
            .map_err(|e| format!("Viking list failed: {}", e))?;

        if resp.status().is_success() {
            resp.json::<Vec<VikingDirEntry>>()
                .await
                .map_err(|e| format!("Viking list parse failed: {}", e))
        } else {
            Err(format!("Viking list returned {}", resp.status()))
        }
    }

    /// Delete a memory entry.
    pub async fn delete_memory(&self, path: &str) -> Result<(), String> {
        let resp = self
            .client
            .delete(format!("{}/api/memory/delete", self.base_url))
            .query(&[("user_id", self.user_id.as_str()), ("path", path)])
            .send()
            .await
            .map_err(|e| format!("Viking delete failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Viking delete returned {}", resp.status()))
        }
    }

    /// Trigger post-session memory self-iteration.
    /// Viking auto-extracts memories into 6 categories from the transcript.
    pub async fn trigger_iteration(&self, session_transcript: &str) -> Result<(), String> {
        let body = serde_json::json!({
            "user_id": self.user_id,
            "transcript": session_transcript,
        });

        let resp = self
            .client
            .post(format!("{}/api/memory/iterate", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Viking iteration failed: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("Viking iteration returned {}", resp.status()))
        }
    }

    /// Get the base URL of this client.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the user ID of this client.
    pub fn user_id(&self) -> &str {
        &self.user_id
    }
}

/// Policy for which context levels to load per layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextLevelPolicy {
    /// Default level for user memories.
    #[serde(default = "default_user_level")]
    pub user_level: ContextLevel,
    /// Default level for agent memories.
    #[serde(default = "default_agent_level")]
    pub agent_level: ContextLevel,
    /// Maximum number of L0 summaries to load.
    #[serde(default = "default_max_l0")]
    pub max_l0_entries: usize,
}

fn default_user_level() -> ContextLevel {
    ContextLevel::L0
}
fn default_agent_level() -> ContextLevel {
    ContextLevel::L0
}
fn default_max_l0() -> usize {
    20
}

impl Default for ContextLevelPolicy {
    fn default() -> Self {
        Self {
            user_level: ContextLevel::L0,
            agent_level: ContextLevel::L0,
            max_l0_entries: 20,
        }
    }
}

/// Load Viking context for injection into the system prompt.
/// Returns formatted context string with L0 summaries relevant to the query.
pub async fn load_viking_context(
    viking: &VikingClient,
    query_hint: &str,
    policy: &ContextLevelPolicy,
) -> String {
    let mut context_parts = Vec::new();

    // Load L0 summaries from user/ directory
    if let Ok(entries) = viking.list_directory("viking://user/").await {
        let mut user_section = String::from("## User Context\n");
        for entry in entries.iter().take(policy.max_l0_entries) {
            if let Some(ref summary) = entry.summary {
                user_section.push_str(&format!("- {}: {}\n", entry.path, summary));
            }
        }
        if !user_section.ends_with("## User Context\n") {
            context_parts.push(user_section);
        }
    }

    // Load L0 summaries from agent/ directory
    if let Ok(entries) = viking.list_directory("viking://agent/").await {
        let mut agent_section = String::from("## Agent Context\n");
        for entry in entries.iter().take(policy.max_l0_entries) {
            if let Some(ref summary) = entry.summary {
                agent_section.push_str(&format!("- {}: {}\n", entry.path, summary));
            }
        }
        if !agent_section.ends_with("## Agent Context\n") {
            context_parts.push(agent_section);
        }
    }

    // Semantic search with user's message as query hint
    if !query_hint.is_empty() {
        if let Ok(results) = viking.search(query_hint, None, 5).await {
            if !results.is_empty() {
                let mut recall_section = String::from("## Recalled Memories\n");
                for result in &results {
                    recall_section.push_str(&format!(
                        "- [score:{:.2}] {}: {}\n",
                        result.relevance_score,
                        result.path,
                        result.content.chars().take(200).collect::<String>()
                    ));
                }
                context_parts.push(recall_section);
            }
        }
    }

    if context_parts.is_empty() {
        return String::new();
    }

    format!(
        "# Sustained Context (Viking)\n\n{}",
        context_parts.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_level_display() {
        assert_eq!(ContextLevel::L0.to_string(), "L0");
        assert_eq!(ContextLevel::L1.to_string(), "L1");
        assert_eq!(ContextLevel::L2.to_string(), "L2");
    }

    #[test]
    fn test_memory_category_paths() {
        assert_eq!(
            MemoryCategory::UserProfile.viking_path(),
            "viking://user/profile/"
        );
        assert_eq!(
            MemoryCategory::AgentPatterns.viking_path(),
            "viking://agent/patterns/"
        );
    }

    #[test]
    fn test_context_level_policy_default() {
        let policy = ContextLevelPolicy::default();
        assert_eq!(policy.user_level, ContextLevel::L0);
        assert_eq!(policy.agent_level, ContextLevel::L0);
        assert_eq!(policy.max_l0_entries, 20);
    }
}
