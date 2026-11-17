//! A small Server-Sent Events reader, for the streaming replies both Anthropic's and OpenAI's APIs
//! send. Bytes go in as they arrive, in whatever pieces the network cuts them; whole events come
//! out. A multi-byte character split across two pieces is held until its other half arrives.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` name, if the server sent one.
    pub event: Option<String>,
    /// The `data:` lines, joined with newlines.
    pub data: String,
}

#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
}

/// Where the next blank line (the end of an event) is: its start, and how long the separator is.
fn boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buffer.len() {
        if buffer[i..].starts_with(b"\n\n") {
            return Some((i, 2));
        }
        if buffer[i..].starts_with(b"\r\n\r\n") {
            return Some((i, 4));
        }
        if buffer[i..].starts_with(b"\r\r") {
            return Some((i, 2));
        }
        i += 1;
    }
    None
}

fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in text.split(['\n', '\r']).filter(|l| !l.is_empty()) {
        if line.starts_with(':') {
            continue; // a comment, often a keep-alive
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => event = Some(value.to_owned()),
            "data" => data.push(value),
            _ => {}
        }
    }
    (!data.is_empty()).then(|| SseEvent { event, data: data.join("\n") })
}

impl SseParser {
    /// Adds what just arrived and returns every event it completed.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some((end, separator)) = boundary(&self.buffer) {
            let block: Vec<u8> = self.buffer.drain(..end + separator).take(end).collect();
            events.extend(parse_block(&block));
        }
        events
    }

    /// The stream ended: an event with no blank line after it still counts.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let rest = std::mem::take(&mut self.buffer);
        parse_block(&rest).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::default();
        let mut out: Vec<SseEvent> = chunks.iter().flat_map(|c| p.push(c)).collect();
        out.extend(p.finish());
        out
    }

    fn ev(event: Option<&str>, data: &str) -> SseEvent {
        SseEvent { event: event.map(str::to_owned), data: data.to_owned() }
    }

    #[test]
    fn events_have_names_and_data() {
        let out = all(&[b"event: message_start\ndata: {\"a\":1}\n\nevent: ping\ndata: {}\n\n"]);
        assert_eq!(out, [ev(Some("message_start"), "{\"a\":1}"), ev(Some("ping"), "{}")]);
    }

    #[test]
    fn events_without_a_name_and_with_several_data_lines() {
        assert_eq!(all(&[b"data: one\ndata: two\n\n"]), [ev(None, "one\ntwo")]);
        assert_eq!(all(&[b"data:no-space\n\n"]), [ev(None, "no-space")]);
    }

    #[test]
    fn however_the_network_cuts_it_the_events_are_the_same() {
        let stream = b"event: a\ndata: {\"t\":\"h\xc3\xa9llo\"}\n\ndata: [DONE]\n\n";
        let whole = all(&[stream]);
        assert_eq!(whole.len(), 2);
        // One byte at a time, including through the middle of the two-byte "é".
        let bytes: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(all(&bytes), whole);
        assert_eq!(whole[0].data, "{\"t\":\"héllo\"}");
    }

    #[test]
    fn windows_line_endings_comments_and_unknown_fields_are_handled() {
        let out = all(&[b": keep-alive\r\n\r\nid: 7\r\nevent: x\r\ndata: y\r\n\r\n"]);
        assert_eq!(out, [ev(Some("x"), "y")]);
    }

    #[test]
    fn a_last_event_with_no_blank_line_still_arrives_and_empty_ones_dont() {
        assert_eq!(all(&[b"data: tail"]), [ev(None, "tail")]);
        assert!(all(&[b"event: only-a-name\n\n"]).is_empty());
        assert!(all(&[b""]).is_empty());
    }
}
