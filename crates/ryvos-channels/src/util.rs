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

    #[test]
    fn message_exactly_at_max_length() {
        let text = "x".repeat(100);
        let result = split_message(&text, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], text);
    }

    #[test]
    fn message_one_byte_over_max() {
        let text = "y".repeat(101);
        let result = split_message(&text, 100);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 100);
        assert_eq!(result[1].len(), 1);
    }

    #[test]
    fn unicode_multibyte_preserved_in_short_message() {
        // Each emoji is 4 bytes
        let text = "\u{1F600}\u{1F601}\u{1F602}"; // 3 emojis = 12 bytes
        let result = split_message(text, 100);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], text);
    }

    #[test]
    fn many_consecutive_newlines() {
        let text = "hello\n\n\n\n\nworld";
        let result = split_message(text, 100);
        // Fits in one chunk
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], text);
    }

    #[test]
    fn many_consecutive_newlines_under_pressure() {
        // Lines separated by empty lines, each line short but many splits
        let text = "a\n\nb\n\nc\n\nd";
        let result = split_message(text, 3);
        // Each chunk should be at most 3 bytes
        for chunk in &result {
            assert!(chunk.len() <= 3, "chunk '{}' exceeds max", chunk);
        }
        // All original content should be recoverable
        let rejoined = result.join("\n");
        // The rejoined text should contain the same non-newline characters
        let orig_chars: String = text.chars().filter(|c| *c != '\n').collect();
        let result_chars: String = rejoined.chars().filter(|c| *c != '\n').collect();
        assert_eq!(orig_chars, result_chars);
    }

    #[test]
    fn newline_only_message() {
        let text = "\n\n\n";
        let result = split_message(text, 10);
        // Should not panic; every chunk should respect max
        for chunk in &result {
            assert!(chunk.len() <= 10);
        }
    }
}
