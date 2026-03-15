use crate::line_buffer::LineBuffer;
use crate::notifier::{FailureNotification, FailureSource, Notifier};
use crate::processor::RunState;

#[derive(Debug)]
pub struct BunStreamProcessor {
    partial_line: LineBuffer,
    in_failures_section: bool,
    summary_total: Option<usize>,
    summary_passed: Option<usize>,
    summary_failed: Option<usize>,
    summary_skipped: Option<usize>,
    summary_todo: Option<usize>,
    state: RunState,
}

impl BunStreamProcessor {
    pub fn new() -> Self {
        Self {
            partial_line: LineBuffer::default(),
            in_failures_section: false,
            summary_total: None,
            summary_passed: None,
            summary_failed: None,
            summary_skipped: None,
            summary_todo: None,
            state: RunState {
                planned: None,
                total: 0,
                passed: 0,
                failed: 0,
                todo: 0,
                skipped: 0,
                bailout_reason: None,
                parse_warning_count: 0,
                protocol_failures: 0,
            },
        }
    }

    pub fn ingest(&mut self, chunk: &str, notifier: &mut dyn Notifier) {
        self.partial_line.push_str(chunk);
        while let Some(line) = self.partial_line.take_next_line() {
            self.process_line(&line, notifier);
        }
    }

    pub fn finish(&mut self, notifier: &mut dyn Notifier) {
        if let Some(line) = self.partial_line.take_remainder() {
            self.process_line(&line, notifier);
        }

        self.apply_summary_overrides();
        notifier.notify_summary(&self.state);
    }

    pub fn into_state(self) -> RunState {
        self.state
    }

    fn process_line(&mut self, line: &str, notifier: &mut dyn Notifier) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            self.in_failures_section = false;
            return;
        }

        if trimmed.eq_ignore_ascii_case("failures:") {
            self.in_failures_section = true;
            return;
        }

        if self.in_failures_section {
            if let Some(label) = parse_failure_section_label(trimmed) {
                self.record_failure(&label, notifier);
                return;
            }

            if parse_summary_count(trimmed).is_none() {
                return;
            }

            self.in_failures_section = false;
        }

        if let Some((kind, value)) = parse_summary_count(trimmed) {
            self.apply_summary_count(kind, value);
            return;
        }

        if let Some(total) = parse_ran_total(trimmed) {
            self.summary_total = Some(total);
            return;
        }

        if let Some(label) = parse_inline_failure_label(trimmed) {
            self.record_failure(&label, notifier);
            return;
        }

        if let Some(dot_counts) = parse_dot_progress(trimmed) {
            self.state.passed += dot_counts.passed;
            self.state.failed += dot_counts.failed;
            self.state.skipped += dot_counts.skipped;
            self.state.total += dot_counts.passed + dot_counts.failed + dot_counts.skipped;
        }
    }

    fn record_failure(&mut self, label: &str, notifier: &mut dyn Notifier) {
        self.state.failed += 1;
        self.state.total += 1;
        notifier.notify_failure(&FailureNotification::new(FailureSource::Bun, label));
    }

    fn apply_summary_count(&mut self, kind: SummaryKind, value: usize) {
        match kind {
            SummaryKind::Total => self.summary_total = Some(value),
            SummaryKind::Passed => self.summary_passed = Some(value),
            SummaryKind::Failed => self.summary_failed = Some(value),
            SummaryKind::Skipped => self.summary_skipped = Some(value),
            SummaryKind::Todo => self.summary_todo = Some(value),
        }
    }

    fn apply_summary_overrides(&mut self) {
        let has_summary = self.summary_total.is_some()
            || self.summary_passed.is_some()
            || self.summary_failed.is_some()
            || self.summary_skipped.is_some()
            || self.summary_todo.is_some();
        if !has_summary {
            return;
        }

        if let Some(value) = self.summary_passed {
            self.state.passed = value;
        }
        if let Some(value) = self.summary_failed {
            self.state.failed = value;
        }
        if let Some(value) = self.summary_skipped {
            self.state.skipped = value;
        }
        if let Some(value) = self.summary_todo {
            self.state.todo = value;
        }

        self.state.total = self.summary_total.unwrap_or(
            self.state.passed + self.state.failed + self.state.skipped + self.state.todo,
        );
    }
}

impl Default for BunStreamProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum SummaryKind {
    Total,
    Passed,
    Failed,
    Skipped,
    Todo,
}

fn parse_inline_failure_label(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix("(fail)") {
        return normalize_inline_label(rest);
    }

    if let Some(rest) = line.strip_prefix("✗") {
        return normalize_inline_label(rest);
    }

    if let Some(rest) = line.strip_prefix('×') {
        return normalize_inline_label(rest);
    }

    None
}

fn normalize_inline_label(rest: &str) -> Option<String> {
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return None;
    }

    let label = trimmed.split_once('[').map(|(left, _)| left.trim_end()).unwrap_or(trimmed).trim();

    if label.is_empty() {
        None
    } else {
        Some(label.to_owned())
    }
}

fn parse_failure_section_label(line: &str) -> Option<String> {
    let candidate = line
        .trim_start_matches(|ch: char| ch == '-' || ch == '*' || ch == '•' || ch.is_whitespace())
        .trim();

    if candidate.is_empty() {
        return None;
    }

    if candidate.ends_with(':') {
        return None;
    }

    if parse_summary_count(candidate).is_some() {
        return None;
    }

    Some(candidate.to_owned())
}

fn parse_ran_total(line: &str) -> Option<usize> {
    let rest = line.strip_prefix("Ran ")?;

    let mut tokens = rest.split_whitespace();
    let total = tokens.next()?.parse::<usize>().ok()?;
    let noun = tokens.next()?;
    if noun.starts_with("test") {
        Some(total)
    } else {
        None
    }
}

fn parse_summary_count(line: &str) -> Option<(SummaryKind, usize)> {
    let mut tokens = line.split_whitespace();
    let count = tokens.next()?.parse::<usize>().ok()?;

    let second = normalize_token(tokens.next()?);
    if let Some(kind) = classify_summary_token(second) {
        return Some((kind, count));
    }

    let third = normalize_token(tokens.next()?);
    classify_summary_token(third).map(|kind| (kind, count))
}

fn normalize_token(token: &str) -> &str {
    token.trim_matches(|ch: char| !ch.is_alphanumeric())
}

fn classify_summary_token(token: &str) -> Option<SummaryKind> {
    if token.starts_with("pass") {
        return Some(SummaryKind::Passed);
    }
    if token.starts_with("fail") {
        return Some(SummaryKind::Failed);
    }
    if token.starts_with("skip") || token.starts_with("pend") {
        return Some(SummaryKind::Skipped);
    }
    if token.starts_with("todo") {
        return Some(SummaryKind::Todo);
    }
    if token.starts_with("test") {
        return Some(SummaryKind::Total);
    }
    None
}

#[derive(Default)]
struct DotCounts {
    passed: usize,
    failed: usize,
    skipped: usize,
}

fn parse_dot_progress(line: &str) -> Option<DotCounts> {
    let mut counts = DotCounts::default();
    let mut saw_progress = false;

    for ch in line.chars() {
        match ch {
            '.' => {
                counts.passed += 1;
                saw_progress = true;
            }
            'F' | 'f' => {
                counts.failed += 1;
                saw_progress = true;
            }
            'S' | 's' => {
                counts.skipped += 1;
                saw_progress = true;
            }
            ch if ch.is_whitespace() => {}
            _ => return None,
        }
    }

    if saw_progress {
        Some(counts)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::notifier::{FailureNotification, Notifier};

    use super::BunStreamProcessor;
    use crate::processor::RunState;

    #[derive(Default)]
    struct RecordingNotifier {
        failures: Vec<FailureNotification>,
        summaries: usize,
    }

    impl Notifier for RecordingNotifier {
        fn notify_failure(&mut self, failure: &FailureNotification) {
            self.failures.push(failure.clone());
        }

        fn notify_bailout(&mut self, _reason: &str) {}

        fn notify_summary(&mut self, _state: &RunState) {
            self.summaries += 1;
        }
    }

    #[test]
    fn parses_inline_failures_and_summary_counts() {
        let input = "bun test v1.2.0\n(pass) works\n(fail) explodes [0.50ms]\n1 pass\n1 fail\nRan 2 tests across 1 file.\n";
        let mut processor = BunStreamProcessor::new();
        let mut notifier = RecordingNotifier::default();

        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);
        let state = processor.into_state();

        assert_eq!(state.total, 2);
        assert_eq!(state.passed, 1);
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures.len(), 1);
        assert_eq!(notifier.failures[0].label, "explodes");
    }

    #[test]
    fn parses_dot_reporter_progress() {
        let input = "..F.S\n3 pass\n1 fail\n1 skip\n";
        let mut processor = BunStreamProcessor::new();
        let mut notifier = RecordingNotifier::default();

        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);
        let state = processor.into_state();

        assert_eq!(state.total, 5);
        assert_eq!(state.passed, 3);
        assert_eq!(state.failed, 1);
        assert_eq!(state.skipped, 1);
    }

    #[test]
    fn parses_failures_section_labels() {
        let input = "failures:\n  should fail hard\n\n1 fail\n";
        let mut processor = BunStreamProcessor::new();
        let mut notifier = RecordingNotifier::default();

        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);

        assert_eq!(notifier.failures.len(), 1);
        assert_eq!(notifier.failures[0].label, "should fail hard");
    }
}
