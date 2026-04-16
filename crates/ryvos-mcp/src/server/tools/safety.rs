use ryvos_agent::SafetyMemory;
use std::sync::Arc;

pub async fn list_lessons(safety: &Arc<SafetyMemory>, search: Option<&str>, limit: usize) -> String {
    let lessons = if let Some(keyword) = search {
        safety.search_lessons(keyword, limit).await
    } else {
        safety.list_lessons(limit, 0).await
    };

    match lessons {
        Ok(lessons) => {
            if lessons.is_empty() {
                "No safety lessons recorded yet.".to_string()
            } else {
                let total = safety.count_lessons().await.unwrap_or(0);
                let mut lines = vec![format!("Safety lessons ({} total, showing {}):", total, lessons.len())];
                for l in &lessons {
                    lines.push(format!(
                        "- [confidence:{:.0}%, applied:{}x] {}\n  Rule: {}\n  Recorded: {}",
                        l.confidence * 100.0,
                        l.times_applied,
                        l.action,
                        l.corrective_rule,
                        l.timestamp.format("%Y-%m-%d %H:%M"),
                    ));
                }
                lines.join("\n")
            }
        }
        Err(e) => format!("Safety lessons query error: {}", e),
    }
}
