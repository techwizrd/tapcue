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
        let newline_idx = remaining.find('\n')?;
        let end = self.start + newline_idx;

        let mut line = self.buffer[self.start..end].to_owned();
        trim_cr(&mut line);

        self.start = end + 1;
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
