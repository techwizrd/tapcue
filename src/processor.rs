use serde::Serialize;

use crate::notifier::Notifier;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunState {
    pub planned: Option<usize>,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub todo: usize,
    pub skipped: usize,
    pub bailout_reason: Option<String>,
    pub parse_warning_count: usize,
}

impl RunState {
    pub fn is_success(&self) -> bool {
        self.failed == 0 && self.bailout_reason.is_none() && !self.has_plan_mismatch()
    }

    pub fn has_plan_mismatch(&self) -> bool {
        match self.planned {
            Some(expected) => expected != self.total,
            None => false,
        }
    }
}

#[derive(Debug)]
pub struct TapStreamProcessor {
    quiet_parse_errors: bool,
    partial_line: String,
    emitted_parse_warning: bool,
    state: RunState,
}

impl TapStreamProcessor {
    pub fn new(quiet_parse_errors: bool) -> Self {
        Self {
            quiet_parse_errors,
            partial_line: String::new(),
            emitted_parse_warning: false,
            state: RunState {
                planned: None,
                total: 0,
                passed: 0,
                failed: 0,
                todo: 0,
                skipped: 0,
                bailout_reason: None,
                parse_warning_count: 0,
            },
        }
    }

    pub fn ingest(&mut self, chunk: &str, notifier: &mut dyn Notifier) {
        self.partial_line.push_str(chunk);

        while let Some(newline_idx) = self.partial_line.find('\n') {
            let mut line = self.partial_line[..newline_idx].to_owned();
            self.partial_line.drain(..=newline_idx);
            if line.ends_with('\r') {
                line.pop();
            }
            self.process_line(&line, notifier);
        }
    }

    pub fn finish(&mut self, notifier: &mut dyn Notifier) {
        if !self.partial_line.is_empty() {
            let mut line = std::mem::take(&mut self.partial_line);
            if line.ends_with('\r') {
                line.pop();
            }
            self.process_line(&line, notifier);
        }
        notifier.notify_summary(&self.state);
    }

    pub fn into_state(self) -> RunState {
        self.state
    }

    fn process_line(&mut self, line: &str, notifier: &mut dyn Notifier) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("TAP version") {
            return;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            return;
        }

        if let Some(reason) = trimmed.strip_prefix("Bail out!") {
            if self.state.bailout_reason.is_none() {
                let reason = reason.trim();
                notifier.notify_bailout(reason);
                self.state.bailout_reason = Some(reason.to_owned());
            }
            return;
        }

        if let Some(planned) = parse_plan(trimmed) {
            self.state.planned = Some(planned);
            return;
        }

        if let Some(test) = parse_test_point(trimmed) {
            self.record_test(test, notifier);
            return;
        }

        self.warn_parse_once(trimmed);
    }

    fn record_test(&mut self, test: ParsedTestPoint<'_>, notifier: &mut dyn Notifier) {
        self.state.total += 1;

        match test.directive {
            Some(ParsedDirective::Todo) => self.state.todo += 1,
            Some(ParsedDirective::Skip) => self.state.skipped += 1,
            None if test.result => self.state.passed += 1,
            None => {
                self.state.failed += 1;
                notifier.notify_failure(test.desc.unwrap_or("unnamed test"));
            }
        }
    }

    fn warn_parse_once(&mut self, message: &str) {
        if self.quiet_parse_errors || self.emitted_parse_warning {
            return;
        }

        self.emitted_parse_warning = true;
        self.state.parse_warning_count += 1;
        eprintln!("tapcue: parse warning: unsupported TAP line: {message}");
    }
}

#[derive(Clone, Copy, Debug)]
enum ParsedDirective {
    Todo,
    Skip,
}

#[derive(Clone, Copy, Debug)]
struct ParsedTestPoint<'a> {
    result: bool,
    desc: Option<&'a str>,
    directive: Option<ParsedDirective>,
}

fn parse_plan(line: &str) -> Option<usize> {
    let (left, right) = line.split_once("..")?;
    if !left.trim().chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    right.trim().parse::<usize>().ok()
}

fn parse_test_point(line: &str) -> Option<ParsedTestPoint<'_>> {
    let (result, remainder) = if line == "ok" {
        (true, "")
    } else if let Some(rest) = line.strip_prefix("ok ") {
        (true, rest)
    } else if line == "not ok" {
        (false, "")
    } else if let Some(rest) = line.strip_prefix("not ok ") {
        (false, rest)
    } else {
        return None;
    };

    let mut body = remainder.trim_start();
    let digits_end = body.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits_end > 0 {
        body = body[digits_end..].trim_start();
    }
    if let Some(rest) = body.strip_prefix('-') {
        body = rest.trim_start();
    }

    let (desc_raw, directive_raw) =
        body.split_once('#').map_or((body, None), |(left, right)| (left, Some(right)));

    let desc = {
        let normalized = desc_raw.trim();
        if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        }
    };

    let directive = directive_raw.and_then(parse_directive);

    Some(ParsedTestPoint { result, desc, directive })
}

fn parse_directive(raw: &str) -> Option<ParsedDirective> {
    let directive = raw.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
    match directive.as_str() {
        "TODO" => Some(ParsedDirective::Todo),
        "SKIP" => Some(ParsedDirective::Skip),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::notifier::Notifier;

    use super::{RunState, TapStreamProcessor};

    #[derive(Debug, Default)]
    struct RecordingNotifier {
        events: Vec<String>,
    }

    impl Notifier for RecordingNotifier {
        fn notify_failure(&mut self, label: &str) {
            self.events.push(format!("failure:{label}"));
        }

        fn notify_bailout(&mut self, reason: &str) {
            self.events.push(format!("bailout:{reason}"));
        }

        fn notify_summary(&mut self, state: &RunState) {
            self.events.push(format!(
                "summary:total={} passed={} failed={} todo={} skipped={} success={}",
                state.total,
                state.passed,
                state.failed,
                state.todo,
                state.skipped,
                state.is_success()
            ));
        }
    }

    fn process_input(input: &str) -> (RunState, RecordingNotifier) {
        let mut processor = TapStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();
        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);
        (processor.into_state(), notifier)
    }

    #[test]
    fn simple_passing_tap() {
        let input = "TAP version 14\n1..2\nok 1 - alpha\nok 2 - beta\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.total, 2);
        assert_eq!(state.passed, 2);
        assert_eq!(state.failed, 0);
        assert!(state.is_success());
        assert_eq!(notifier.events.len(), 1);
        assert!(notifier.events[0].starts_with("summary:"));
    }

    #[test]
    fn failing_test_point_sends_notification() {
        let input = "TAP version 14\n1..1\nnot ok 1 - boom\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.failed, 1);
        assert!(!state.is_success());
        assert!(notifier.events.contains(&"failure:boom".to_owned()));
    }

    #[test]
    fn todo_failures_are_not_counted_as_failures() {
        let input = "TAP version 14\n1..1\nnot ok 1 - pending # TODO flaky\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.failed, 0);
        assert_eq!(state.todo, 1);
        assert!(state.is_success());
        assert!(!notifier.events.iter().any(|event| event.starts_with("failure:")));
    }

    #[test]
    fn skip_directives_are_not_failures() {
        let input = "TAP version 14\n1..2\nok 1 - skipped # SKIP not relevant\nnot ok 2 - odd # SKIP broken harness\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.failed, 0);
        assert_eq!(state.skipped, 2);
        assert!(state.is_success());
        assert!(!notifier.events.iter().any(|event| event.starts_with("failure:")));
    }

    #[test]
    fn bailout_is_recorded_once() {
        let mut processor = TapStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version 14\n1..2\nBail out! stop now", &mut notifier);
        processor.ingest("\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.bailout_reason, Some("stop now".to_owned()));

        let bailout_events =
            notifier.events.iter().filter(|event| event.starts_with("bailout:")).count();
        assert_eq!(bailout_events, 1);
    }

    #[test]
    fn completion_summary_reflects_plan_mismatch() {
        let input = "TAP version 14\n1..2\nok 1 - only\n";
        let (state, notifier) = process_input(input);

        assert!(state.has_plan_mismatch());
        assert!(!state.is_success());
        assert!(notifier.events.iter().any(
            |event| event == "summary:total=1 passed=1 failed=0 todo=0 skipped=0 success=false"
        ));
    }

    #[test]
    fn streaming_incremental_lines() {
        let mut processor = TapStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version", &mut notifier);
        processor.ingest(" 14\n1..2\n", &mut notifier);
        processor.ingest("ok 1 - first\n", &mut notifier);
        processor.ingest("not ok 2 - second\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);

        let failure_events =
            notifier.events.iter().filter(|event| event.starts_with("failure:")).count();
        assert_eq!(failure_events, 1);
    }

    #[test]
    fn malformed_then_valid_tap_is_processed() {
        let mut processor = TapStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version 14\n1..1\nnot", &mut notifier);
        processor.ingest(" ok 1 - later valid\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.failed, 1);
        assert!(notifier.events.iter().any(|event| event == "failure:later valid"));
    }

    #[test]
    fn subtest_input_is_counted_via_ending_test() {
        let input =
            "TAP version 14\n# Subtest: math\n    1..1\n    ok 1 - add\nok 1 - math\n1..1\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.total, 1);
        assert_eq!(state.failed, 0);
        assert_eq!(state.passed, 1);
        assert!(state.is_success());
        assert_eq!(notifier.events.len(), 1);
    }

    #[test]
    fn reparsing_only_processes_new_statements() {
        let mut processor = TapStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version 14\n1..2\nnot ok 1 - first\n", &mut notifier);
        processor.ingest("# comment that should not duplicate\n", &mut notifier);
        processor.ingest("ok 2 - second\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);

        let first_failures =
            notifier.events.iter().filter(|event| *event == "failure:first").count();
        assert_eq!(first_failures, 1);
    }
}
