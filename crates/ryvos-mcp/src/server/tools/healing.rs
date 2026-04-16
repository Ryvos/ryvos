use ryvos_agent::FailureJournal;
use std::sync::Arc;

pub async fn query_decisions(
    journal: &Arc<FailureJournal>,
    session_id: Option<&str>,
    limit: usize,
) -> String {
    match journal.list_decisions(limit, 0) {
        Ok(decisions) => {
            let filtered: Vec<_> = if let Some(sid) = session_id {
                decisions
                    .into_iter()
                    .filter(|d| d.session_id.starts_with(sid))
                    .collect()
            } else {
                decisions
            };
            if filtered.is_empty() {
                "No decisions recorded yet.".to_string()
            } else {
                let total = journal.count_decisions().unwrap_or(0);
                let mut lines = vec![format!(
                    "Agent decisions ({} total, showing {}):",
                    total,
                    filtered.len()
                )];
                for d in &filtered {
                    let alts: Vec<_> = d.alternatives.iter().map(|a| a.name.as_str()).collect();
                    lines.push(format!(
                        "- [turn:{}, session:{}] {}\n  Chose: {} | Alternatives: {}",
                        d.turn,
                        &d.session_id[..8.min(d.session_id.len())],
                        d.description,
                        d.chosen_option,
                        if alts.is_empty() {
                            "none".to_string()
                        } else {
                            alts.join(", ")
                        },
                    ));
                }
                lines.join("\n")
            }
        }
        Err(e) => format!("Decision query error: {}", e),
    }
}

pub async fn query_failures(
    journal: &Arc<FailureJournal>,
    pattern: Option<&str>,
    tool: Option<&str>,
    limit: usize,
) -> String {
    match journal.search_failures(pattern, tool, limit) {
        Ok(failures) => {
            if failures.is_empty() {
                "No failures recorded yet.".to_string()
            } else {
                let total = journal.count_failures().unwrap_or(0);
                let mut lines = vec![format!(
                    "Failure journal ({} total, showing {}):",
                    total,
                    failures.len()
                )];
                for f in &failures {
                    let error_preview = if f.error.len() > 100 {
                        format!("{}...", &f.error[..100])
                    } else {
                        f.error.clone()
                    };
                    lines.push(format!(
                        "- [{}] tool:{} turn:{}\n  Error: {}",
                        f.timestamp.format("%Y-%m-%d %H:%M"),
                        f.tool_name,
                        f.turn,
                        error_preview,
                    ));
                }
                lines.join("\n")
            }
        }
        Err(e) => format!("Failure query error: {}", e),
    }
}
