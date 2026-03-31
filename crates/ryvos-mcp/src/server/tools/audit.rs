use super::super::audit_reader::AuditReader;
use std::sync::Arc;

pub async fn query(audit: &Arc<AuditReader>, limit: usize) -> String {
    match audit.recent_entries(limit).await {
        Ok(entries) => {
            if entries.is_empty() {
                "No audit entries found.".to_string()
            } else {
                entries
                    .iter()
                    .map(|e| {
                        format!(
                            "[{}] {} — {} | outcome: {}",
                            e.timestamp, e.tool_name, e.input_summary, e.outcome
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        Err(e) => format!("Audit query error: {}", e),
    }
}

pub async fn stats(audit: &Arc<AuditReader>) -> String {
    match audit.tool_counts().await {
        Ok(counts) => {
            if counts.is_empty() {
                "No audit data yet.".to_string()
            } else {
                let total: u64 = counts.iter().map(|(_, c)| c).sum();
                let mut lines = vec![format!("Total tool calls: {}", total)];
                for (name, count) in &counts {
                    lines.push(format!("  {}: {} calls", name, count));
                }
                lines.join("\n")
            }
        }
        Err(e) => format!("Audit stats error: {}", e),
    }
}
