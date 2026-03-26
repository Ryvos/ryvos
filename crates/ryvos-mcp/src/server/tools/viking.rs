use std::sync::Arc;
use ryvos_memory::VikingClient;
use ryvos_memory::viking::{VikingMeta, ContextLevel};

pub async fn search(viking: &Arc<VikingClient>, query: &str, directory: Option<&str>, limit: usize) -> String {
    match viking.search(query, directory, limit).await {
        Ok(results) => {
            if results.is_empty() {
                "No results found.".to_string()
            } else {
                results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        format!(
                            "{}. [{}] (score: {:.2})\n   {}",
                            i + 1,
                            r.path,
                            r.relevance_score,
                            if r.content.len() > 200 { format!("{}...", &r.content[..200]) } else { r.content.clone() }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        }
        Err(e) => format!("Viking search error: {}", e),
    }
}

pub async fn read(viking: &Arc<VikingClient>, path: &str, level: &str) -> String {
    let ctx_level = match level {
        "L0" | "l0" => ContextLevel::L0,
        "L2" | "l2" => ContextLevel::L2,
        _ => ContextLevel::L1,
    };
    match viking.read_memory(path, ctx_level).await {
        Ok(result) => {
            if result.content.is_empty() {
                format!("No content at {} (level {})", path, level)
            } else {
                result.content
            }
        }
        Err(e) => format!("Viking read error: {}", e),
    }
}

pub async fn write(viking: &Arc<VikingClient>, path: &str, content: &str, tags: Option<&[String]>) -> String {
    let meta = tags.map(|t| VikingMeta {
        tags: t.to_vec(),
        ..Default::default()
    });
    match viking.write_memory(path, content, meta).await {
        Ok(()) => format!("Written to {}", path),
        Err(e) => format!("Viking write error: {}", e),
    }
}

pub async fn list(viking: &Arc<VikingClient>, path: &str) -> String {
    match viking.list_directory(path).await {
        Ok(entries) => {
            if entries.is_empty() {
                format!("Empty directory: {}", path)
            } else {
                entries
                    .iter()
                    .map(|e| {
                        let icon = if e.is_directory { "📁" } else { "📄" };
                        format!("{} {} — {}", icon, e.path, e.summary.as_deref().unwrap_or(""))
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        Err(e) => format!("Viking list error: {}", e),
    }
}
