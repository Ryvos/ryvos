/// Split a message into chunks that fit within `max_len`.
///
/// Splits on newline boundaries when possible, falling back to
/// hard splits at `max_len` if a single line exceeds the limit.
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.split('\n') {
        // If adding this line (plus newline) would exceed the limit
        let needed = if current.is_empty() {
            line.len()
        } else {
            current.len() + 1 + line.len()
        };

        if needed > max_len {
            // Flush current chunk if non-empty
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }

            // If the line itself exceeds max_len, hard-split it
            if line.len() > max_len {
                let mut remaining = line;
                while remaining.len() > max_len {
                    chunks.push(remaining[..max_len].to_string());
                    remaining = &remaining[max_len..];
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = line.to_string();
            }
        } else if current.is_empty() {
            current = line.to_string();
        } else {
            current.push('\n');
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_message_unchanged() {
        let result = split_message("hello", 100);
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn empty_message() {
        let result = split_message("", 100);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn split_on_newlines() {
        let text = "line1\nline2\nline3";
        let result = split_message(text, 11);
        assert_eq!(result, vec!["line1\nline2", "line3"]);
    }

    #[test]
    fn hard_split_long_line() {
        let text = "a".repeat(25);
        let result = split_message(&text, 10);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].len(), 10);
        assert_eq!(result[1].len(), 10);
        assert_eq!(result[2].len(), 5);
    }

    #[test]
    fn telegram_limit() {
        let text = "a".repeat(5000);
        let result = split_message(&text, 4096);
        assert!(result.len() >= 2);
        for chunk in &result {
            assert!(chunk.len() <= 4096);
        }
    }
}
