use std::path::Path;

pub fn get(workspace: &Path, name: Option<&str>) -> String {
    let file_name = name.unwrap_or("MEMORY");
    let path = if file_name == "MEMORY" {
        workspace.join("MEMORY.md")
    } else {
        workspace.join("memory").join(format!("{}.md", file_name))
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => format!("Could not read {}: {}", path.display(), e),
    }
}

pub fn write(workspace: &Path, note: &str) -> String {
    let memory_path = workspace.join("MEMORY.md");
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let entry = format!("\n- **{}** — {}\n", timestamp, note);

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&memory_path)
    {
        Ok(mut file) => {
            use std::io::Write;
            match file.write_all(entry.as_bytes()) {
                Ok(()) => format!("Written to MEMORY.md: {}", note),
                Err(e) => format!("Write error: {}", e),
            }
        }
        Err(e) => format!("Could not open MEMORY.md: {}", e),
    }
}

pub fn daily_log_write(workspace: &Path, entry: &str) -> String {
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let time = chrono::Utc::now().format("%H:%M").to_string();
    let log_path = workspace.join("memory").join(format!("{}.md", today));

    // Ensure memory directory exists
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let line = format!("- **{} UTC** — {}\n", time, entry);

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(mut file) => {
            use std::io::Write;
            match file.write_all(line.as_bytes()) {
                Ok(()) => format!("Logged to {}.md", today),
                Err(e) => format!("Write error: {}", e),
            }
        }
        Err(e) => format!("Could not open daily log: {}", e),
    }
}
