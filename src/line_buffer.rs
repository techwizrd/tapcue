#[derive(Debug, Default)]
pub(crate) struct LineBuffer {
    buffer: String,
    start: usize,
}

impl LineBuffer {
    pub(crate) fn push_str(&mut self, chunk: &str) {
        self.buffer.push_str(chunk);
    }

    pub(crate) fn take_next_line(&mut self) -> Option<String> {
        let remaining = &self.buffer[self.start..];
        let (line_end_idx, separator_len) = find_line_separator(remaining)?;
        let end = self.start + line_end_idx;

        let mut line = self.buffer[self.start..end].to_owned();
        trim_cr(&mut line);

        self.start = end + separator_len;
        self.compact_if_needed();
        Some(line)
    }

    pub(crate) fn take_remainder(&mut self) -> Option<String> {
        if self.start >= self.buffer.len() {
            self.buffer.clear();
            self.start = 0;
            return None;
        }

        let mut remainder = if self.start == 0 {
            std::mem::take(&mut self.buffer)
        } else {
            self.buffer[self.start..].to_owned()
        };

        self.buffer.clear();
        self.start = 0;
        trim_cr(&mut remainder);
        Some(remainder)
    }

    fn compact_if_needed(&mut self) {
        const COMPACT_THRESHOLD: usize = 4096;
        if self.start > COMPACT_THRESHOLD && self.start * 2 >= self.buffer.len() {
            self.buffer.drain(..self.start);
            self.start = 0;
        }
    }
}

pub(crate) fn trim_cr(line: &mut String) {
    if line.ends_with('\r') {
        line.pop();
    }
}

fn find_line_separator(input: &str) -> Option<(usize, usize)> {
    let bytes = input.as_bytes();
    let idx = bytes.iter().position(|byte| *byte == b'\n' || *byte == b'\r')?;

    if bytes[idx] == b'\r' && bytes.get(idx + 1) == Some(&b'\n') {
        Some((idx, 2))
    } else {
        Some((idx, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::LineBuffer;

    #[test]
    fn splits_lf_lines() {
        let mut buffer = LineBuffer::default();
        buffer.push_str("a\nb\n");

        assert_eq!(buffer.take_next_line().as_deref(), Some("a"));
        assert_eq!(buffer.take_next_line().as_deref(), Some("b"));
        assert_eq!(buffer.take_next_line(), None);
    }

    #[test]
    fn splits_crlf_lines() {
        let mut buffer = LineBuffer::default();
        buffer.push_str("a\r\nb\r\n");

        assert_eq!(buffer.take_next_line().as_deref(), Some("a"));
        assert_eq!(buffer.take_next_line().as_deref(), Some("b"));
        assert_eq!(buffer.take_next_line(), None);
    }

    #[test]
    fn splits_cr_only_lines() {
        let mut buffer = LineBuffer::default();
        buffer.push_str("a\rb\r");

        assert_eq!(buffer.take_next_line().as_deref(), Some("a"));
        assert_eq!(buffer.take_next_line().as_deref(), Some("b"));
        assert_eq!(buffer.take_next_line(), None);
    }
}
