use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Parse a raw SSE byte stream into individual events.
/// SSE format: `event: <type>\ndata: <json>\n\n`
#[derive(Default)]
pub struct SseParser {
    buffer: String,
}

/// A parsed SSE event.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: Option<String>,
    pub data: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes into the parser and extract complete events.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        // Split on double newlines (event boundaries)
        while let Some(pos) = self.buffer.find("\n\n") {
            let block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            let mut event_type = None;
            let mut data_lines = Vec::new();

            for line in block.lines() {
                if let Some(val) = line.strip_prefix("event: ") {
                    event_type = Some(val.to_string());
                } else if let Some(val) = line.strip_prefix("data: ") {
                    data_lines.push(val.to_string());
                } else if let Some(val) = line.strip_prefix("data:") {
                    // data with no space after colon
                    data_lines.push(val.to_string());
                }
            }

            if !data_lines.is_empty() {
                events.push(SseEvent {
                    event_type,
                    data: data_lines.join("\n"),
                });
            }
        }

        events
    }
}

/// A stream of SSE events from raw bytes.
pub struct SseStream<S> {
    inner: S,
    parser: SseParser,
    pending: Vec<SseEvent>,
}

impl<S> SseStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            parser: SseParser::new(),
            pending: Vec::new(),
        }
    }
}

impl<S> Stream for SseStream<S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    type Item = SseEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        // Return pending events first
        if !this.pending.is_empty() {
            return Poll::Ready(Some(this.pending.remove(0)));
        }

        // Poll inner stream for more bytes
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let mut events = this.parser.feed(text);
                    if events.is_empty() {
                        // Need more data, wake again
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    } else {
                        let first = events.remove(0);
                        this.pending = events;
                        Poll::Ready(Some(first))
                    }
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Ready(Some(Err(_))) => Poll::Ready(None),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parser_basic() {
        let mut parser = SseParser::new();
        let events = parser.feed("event: message_start\ndata: {\"type\":\"message_start\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type.as_deref(), Some("message_start"));
        assert_eq!(events[0].data, "{\"type\":\"message_start\"}");
    }

    #[test]
    fn test_sse_parser_multiple_events() {
        let mut parser = SseParser::new();
        let events = parser.feed("event: a\ndata: {\"x\":1}\n\nevent: b\ndata: {\"x\":2}\n\n");
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_sse_parser_chunked() {
        let mut parser = SseParser::new();
        let events = parser.feed("event: a\ndata: {\"x\":");
        assert_eq!(events.len(), 0);
        let events = parser.feed("1}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "{\"x\":1}");
    }
}
