use std::borrow::Cow;
use std::collections::HashSet;

use serde::Serialize;

use crate::line_buffer::LineBuffer;
use crate::notifier::{FailureNotification, FailureSource, Notifier};

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
    pub protocol_failures: usize,
}

impl RunState {
    pub fn is_success(&self) -> bool {
        self.failed == 0
            && self.bailout_reason.is_none()
            && !self.has_plan_mismatch()
            && self.protocol_failures == 0
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
    partial_line: LineBuffer,
    strict_mode: bool,
    strict_enforced: bool,
    in_yaml_block: bool,
    yaml_can_start: bool,
    capturing_subtest: bool,
    subtest_lines: Vec<String>,
    pending_subtest_state: Option<RunState>,
    seen_version_line: bool,
    seen_non_version_content: bool,
    seen_plan: bool,
    plan_is_trailing: bool,
    seen_test_point: bool,
    test_counter: usize,
    plan_start: Option<usize>,
    plan_end: Option<usize>,
    seen_test_ids: HashSet<usize>,
    state: RunState,
}

impl TapStreamProcessor {
    pub fn new(quiet_parse_errors: bool, strict_mode: bool) -> Self {
        Self {
            quiet_parse_errors,
            partial_line: LineBuffer::default(),
            strict_mode,
            strict_enforced: strict_mode,
            in_yaml_block: false,
            yaml_can_start: false,
            capturing_subtest: false,
            subtest_lines: Vec::new(),
            pending_subtest_state: None,
            seen_version_line: false,
            seen_non_version_content: false,
            seen_plan: false,
            plan_is_trailing: false,
            seen_test_point: false,
            test_counter: 0,
            plan_start: None,
            plan_end: None,
            seen_test_ids: HashSet::new(),
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

        if self.capturing_subtest {
            self.finalize_subtest();
        }

        if self.pending_subtest_state.is_some() {
            self.protocol_failure("subtest must be followed by parent test point");
            self.pending_subtest_state = None;
        }

        if !self.seen_plan {
            self.protocol_failure("missing TAP plan");
        }

        if self.state.planned == Some(0) && self.state.total > 0 {
            self.protocol_failure("plan 1..0 cannot include test points");
        }

        if let (Some(start), Some(end)) = (self.plan_start, self.plan_end) {
            for id in &self.seen_test_ids {
                if *id < start || *id > end {
                    self.protocol_failure("test point id out of plan range");
                    break;
                }
            }
        }

        notifier.notify_summary(&self.state);
    }

    pub fn into_state(self) -> RunState {
        self.state
    }

    fn process_line(&mut self, line: &str, notifier: &mut dyn Notifier) {
        let trimmed = line.trim();

        if self.capturing_subtest {
            if let Some(nested) = line.strip_prefix("    ") {
                self.subtest_lines.push(nested.to_owned());
                return;
            }

            self.finalize_subtest();
        }

        if self.in_yaml_block {
            if line.starts_with("  ...") {
                self.in_yaml_block = false;
                return;
            }

            if line.starts_with("  ") {
                return;
            }

            self.in_yaml_block = false;
            self.parse_warning("invalid YAML diagnostic line", trimmed);
            if self.strict_mode {
                self.protocol_failure("invalid TAP line under strict mode");
            }
            return;
        }

        if trimmed.is_empty() {
            return;
        }

        if let Some(version_valid) = parse_version_line(trimmed) {
            if self.seen_version_line {
                self.protocol_failure("duplicate TAP version line");
            }

            if self.seen_non_version_content {
                self.protocol_failure("TAP version line must be the first non-empty line");
            }

            self.seen_version_line = true;

            if !version_valid {
                self.parse_warning("unsupported TAP version", trimmed);
                if self.strict_mode {
                    self.protocol_failure("invalid TAP line under strict mode");
                }
            }
            return;
        }

        self.seen_non_version_content = true;

        if self.yaml_can_start {
            if line.starts_with("  ---") {
                self.in_yaml_block = true;
                self.yaml_can_start = false;
                return;
            }
            self.yaml_can_start = false;
        }

        if trimmed.starts_with('#') {
            if self.pending_subtest_state.is_some() {
                self.protocol_failure("subtest must be followed by parent test point");
                self.pending_subtest_state = None;
            }

            if trimmed.starts_with("# Subtest:") {
                self.capturing_subtest = true;
                self.subtest_lines.clear();
            }
            return;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            self.parse_warning("unexpected indented TAP line", trimmed);
            if self.strict_mode {
                self.protocol_failure("invalid TAP line under strict mode");
            }
            return;
        }

        if self.pending_subtest_state.is_some()
            && !trimmed.starts_with("ok")
            && !trimmed.starts_with("not ok")
        {
            self.protocol_failure("subtest must be followed by parent test point");
            self.pending_subtest_state = None;
        }

        if let Some(reason) = parse_bailout(trimmed) {
            if self.state.bailout_reason.is_none() {
                notifier.notify_bailout(reason.as_ref());
                self.state.bailout_reason = Some(reason.into_owned());
            }
            return;
        }

        if let Some(pragma) = parse_pragma(trimmed) {
            if pragma.key.eq_ignore_ascii_case("strict") && !self.strict_enforced {
                self.strict_mode = pragma.enabled;
            }
            return;
        }

        if let Some(plan) = parse_plan(trimmed) {
            self.apply_plan(plan);
            return;
        }

        if self.seen_plan && self.plan_is_trailing {
            self.protocol_failure("test point found after trailing plan");
            return;
        }

        if let Some(test) = parse_test_point(trimmed) {
            if let Some(subtest_state) = self.pending_subtest_state.take() {
                self.correlate_subtest_with_parent(&subtest_state, &test);
            }
            self.record_test(test, notifier);
            self.yaml_can_start = true;
            return;
        }

        self.parse_warning("unsupported TAP line", trimmed);
        if self.strict_mode {
            self.protocol_failure("invalid TAP line under strict mode");
        }
    }

    fn apply_plan(&mut self, plan: ParsedPlan) {
        if self.seen_plan {
            self.protocol_failure("multiple TAP plans are not allowed");
            return;
        }

        self.seen_plan = true;
        self.plan_start = Some(plan.start);
        self.plan_end = Some(plan.end);
        self.plan_is_trailing = self.seen_test_point;

        if plan.start != 1 {
            self.protocol_failure("TAP14 plans must start at 1");
        }

        if plan.start == 1 && plan.end == 0 {
            self.state.planned = Some(0);
            return;
        }

        if plan.start > plan.end {
            self.protocol_failure("invalid TAP plan range");
            return;
        }

        self.state.planned = Some(plan.end.saturating_sub(plan.start) + 1);
    }

    fn record_test(&mut self, test: ParsedTestPoint<'_>, notifier: &mut dyn Notifier) {
        self.seen_test_point = true;

        let id = test.id.unwrap_or(self.test_counter.saturating_add(1));
        self.test_counter = id;
        if !self.seen_test_ids.insert(id) {
            self.protocol_failure("reused test point id");
        }

        self.state.total += 1;

        match test.directive {
            Some(ParsedDirective::Todo) => {
                self.state.todo += 1;
            }
            Some(ParsedDirective::Skip) => {
                self.state.skipped += 1;
            }
            None if test.result => {
                self.state.passed += 1;
            }
            None => {
                self.state.failed += 1;
                let label = test.desc.as_deref().unwrap_or("unnamed test");
                let mut failure = FailureNotification::new(FailureSource::Tap, label);
                failure.reason = Some(label.to_owned());
                notifier.notify_failure(&failure);
            }
        }
    }

    fn parse_warning(&mut self, message: &str, detail: &str) {
        self.state.parse_warning_count += 1;
        if !self.quiet_parse_errors {
            eprintln!("tapcue: parse warning: {message}: {detail}");
        }
    }

    fn protocol_failure(&mut self, reason: &str) {
        self.state.protocol_failures += 1;
        if !self.quiet_parse_errors {
            eprintln!("tapcue: protocol failure: {reason}");
        }
    }

    fn finalize_subtest(&mut self) {
        self.capturing_subtest = false;
        let nested_stream = std::mem::take(&mut self.subtest_lines);

        let mut nested_processor =
            TapStreamProcessor::new(self.quiet_parse_errors, self.strict_enforced);
        nested_processor.strict_mode = self.strict_mode;

        let mut sink = NoopNotifier;
        for line in nested_stream {
            nested_processor.ingest(&line, &mut sink);
            nested_processor.ingest("\n", &mut sink);
        }
        nested_processor.finish(&mut sink);

        let nested_state = nested_processor.into_state();
        self.state.parse_warning_count += nested_state.parse_warning_count;
        self.state.protocol_failures += nested_state.protocol_failures;
        if nested_state.bailout_reason.is_some() {
            self.protocol_failure("nested subtest bailed out");
        }

        self.pending_subtest_state = Some(nested_state);
    }

    fn correlate_subtest_with_parent(&mut self, subtest: &RunState, parent: &ParsedTestPoint<'_>) {
        if parent.directive.is_some() {
            return;
        }

        let nested_success = subtest.is_success();
        if parent.result != nested_success {
            self.protocol_failure("subtest parent result contradicts nested outcome");
        }
    }
}

#[derive(Debug)]
struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify_failure(&mut self, _failure: &FailureNotification) {}

    fn notify_bailout(&mut self, _reason: &str) {}

    fn notify_summary(&mut self, _state: &RunState) {}
}

#[derive(Clone, Copy, Debug)]
struct ParsedPlan {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy, Debug)]
struct ParsedPragma<'a> {
    enabled: bool,
    key: &'a str,
}

#[derive(Clone, Copy, Debug)]
enum ParsedDirective {
    Todo,
    Skip,
}

#[derive(Clone, Debug)]
struct ParsedTestPoint<'a> {
    result: bool,
    id: Option<usize>,
    desc: Option<Cow<'a, str>>,
    directive: Option<ParsedDirective>,
}

fn parse_plan(line: &str) -> Option<ParsedPlan> {
    let (left, right) = line.split_once("..")?;
    let start = left.trim().parse::<usize>().ok()?;
    let end_raw = right.split_once(" #").map_or(right, |(count, _reason)| count).trim();
    let end = end_raw.parse::<usize>().ok()?;
    Some(ParsedPlan { start, end })
}

fn parse_version_line(line: &str) -> Option<bool> {
    if !line.starts_with("TAP version ") {
        return None;
    }

    let version = line.trim_start_matches("TAP version ").trim();
    Some(version == "14" || version == "13")
}

fn parse_bailout(line: &str) -> Option<Cow<'_, str>> {
    let prefix = "Bail out!";
    let head = line.get(..prefix.len())?;

    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }

    let reason = line.get(prefix.len()..).unwrap_or("").trim();
    Some(unescape_text(reason))
}

fn parse_pragma(line: &str) -> Option<ParsedPragma<'_>> {
    let remainder = line.strip_prefix("pragma ")?;
    let (sigil, key) = remainder.split_at(1);
    let enabled = match sigil {
        "+" => true,
        "-" => false,
        _ => return None,
    };

    if key.is_empty() || !key.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }

    Some(ParsedPragma { enabled, key })
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
    let id = {
        let digits_end = body.chars().take_while(|ch| ch.is_ascii_digit()).count();
        if digits_end > 0 {
            let parsed = body[..digits_end].parse::<usize>().ok();
            body = body[digits_end..].trim_start();
            parsed
        } else {
            None
        }
    };

    if let Some(rest) = body.strip_prefix('-') {
        body = rest.trim_start();
    }

    let (desc_raw, directive_raw) = split_directive(body);
    let directive = directive_raw.and_then(parse_directive);

    let desc = if directive.is_some() {
        normalize_description(desc_raw)
    } else {
        normalize_description(body)
    };

    Some(ParsedTestPoint { result, id, desc, directive })
}

fn split_directive(body: &str) -> (&str, Option<&str>) {
    let bytes = body.as_bytes();
    let mut escaped = false;

    for idx in 0..bytes.len() {
        let current = bytes[idx];
        if current == b'\\' {
            escaped = !escaped;
            continue;
        }

        if current == b'#' && !escaped {
            let prev_ok = idx > 0 && body[..idx].ends_with(char::is_whitespace);
            let next_ok = body[idx + 1..].starts_with(char::is_whitespace);
            if prev_ok && next_ok {
                return (&body[..idx], Some(&body[idx + 1..]));
            }
        }

        escaped = false;
    }

    (body, None)
}

fn parse_directive(raw: &str) -> Option<ParsedDirective> {
    let token = raw.split_whitespace().next()?;
    if token.eq_ignore_ascii_case("TODO") {
        Some(ParsedDirective::Todo)
    } else if token.eq_ignore_ascii_case("SKIP") {
        Some(ParsedDirective::Skip)
    } else {
        None
    }
}

fn normalize_description(raw: &str) -> Option<Cow<'_, str>> {
    let unescaped = unescape_text(raw.trim());
    if unescaped.is_empty() {
        None
    } else {
        Some(unescaped)
    }
}

fn unescape_text(input: &str) -> Cow<'_, str> {
    let trimmed = input.trim();
    if !trimmed.as_bytes().contains(&b'\\') {
        return Cow::Borrowed(trimmed);
    }

    let mut out = String::with_capacity(trimmed.len());
    let mut chars = trimmed.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('#') => out.push('#'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }

    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use crate::notifier::{FailureNotification, Notifier};

    use super::{RunState, TapStreamProcessor};

    #[derive(Debug, Default)]
    struct RecordingNotifier {
        failures: Vec<FailureNotification>,
        events: Vec<String>,
    }

    impl Notifier for RecordingNotifier {
        fn notify_failure(&mut self, failure: &FailureNotification) {
            self.failures.push(failure.clone());
            self.events.push(format!("failure:{}", failure.label));
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
        let mut processor = TapStreamProcessor::new(false, false);
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
    }

    #[test]
    fn failing_test_point_sends_notification() {
        let input = "TAP version 14\n1..1\nnot ok 1 - boom\n";
        let (state, notifier) = process_input(input);

        assert_eq!(state.failed, 1);
        assert!(!state.is_success());
        assert!(notifier.events.contains(&"failure:boom".to_owned()));
        assert_eq!(notifier.failures[0].source.as_str(), "TAP");
        assert_eq!(notifier.failures[0].reason.as_deref(), Some("boom"));
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
        let mut processor = TapStreamProcessor::new(false, false);
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
        let mut processor = TapStreamProcessor::new(false, false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version", &mut notifier);
        processor.ingest(" 14\n1..2\n", &mut notifier);
        processor.ingest("ok 1 - first\n", &mut notifier);
        processor.ingest("not ok 2 - second\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);
    }

    #[test]
    fn malformed_then_valid_tap_is_processed() {
        let mut processor = TapStreamProcessor::new(false, false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("TAP version 14\n1..1\nnot", &mut notifier);
        processor.ingest(" ok 1 - later valid\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.failed, 1);
        assert!(notifier.events.iter().any(|event| event == "failure:later valid"));
        assert_eq!(notifier.failures[0].reason.as_deref(), Some("later valid"));
    }

    #[test]
    fn subtest_input_is_counted_via_ending_test() {
        let input =
            "TAP version 14\n# Subtest: math\n    1..1\n    ok 1 - add\nok 1 - math\n1..1\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.total, 1);
        assert_eq!(state.passed, 1);
        assert!(state.is_success());
    }

    #[test]
    fn strict_pragma_converts_invalid_line_to_failure() {
        let input = "TAP version 14\npragma +strict\n1..1\nthis is invalid\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.protocol_failures, 1);
        assert!(!state.is_success());
    }

    #[test]
    fn enforced_strict_mode_ignores_pragma_minus_strict() {
        let input = "TAP version 14\npragma -strict\n1..1\nthis is invalid\nok 1 - still counted\n";
        let mut processor = TapStreamProcessor::new(false, true);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.protocol_failures, 1);
        assert!(!state.is_success());
    }

    #[test]
    fn missing_plan_is_protocol_failure() {
        let input = "TAP version 14\nok 1 - no plan\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.protocol_failures, 1);
        assert!(!state.is_success());
    }

    #[test]
    fn invalid_version_is_warning_and_strict_can_fail() {
        let input = "TAP version 12\n1..1\nok 1 - only\n";
        let (state, _notifier) = process_input(input);
        assert_eq!(state.parse_warning_count, 1);
        assert_eq!(state.protocol_failures, 0);

        let strict_input = "pragma +strict\nTAP version 12\n1..1\nok 1 - only\n";
        let (strict_state, _notifier) = process_input(strict_input);
        assert_eq!(strict_state.protocol_failures, 2);
    }

    #[test]
    fn plan_must_start_at_one_and_duplicate_plans_fail() {
        let input = "TAP version 14\n2..3\nok 2 - bad range\nok 3 - bad range\n";
        let (state, _notifier) = process_input(input);
        assert_eq!(state.protocol_failures, 1);

        let duplicate_plan = "TAP version 14\n1..1\nok 1 - first\n1..1\n";
        let (dup_state, _notifier) = process_input(duplicate_plan);
        assert_eq!(dup_state.protocol_failures, 1);
    }

    #[test]
    fn escaped_hash_does_not_trigger_directive() {
        let input = "TAP version 14\n1..1\nnot ok 1 - desc \\# TODO not directive\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.todo, 0);
        assert_eq!(state.failed, 1);
    }

    #[test]
    fn escaped_sequences_are_unescaped_in_labels_and_bailouts() {
        let mut processor = TapStreamProcessor::new(false, false);
        let mut notifier = RecordingNotifier::default();

        processor
            .ingest("TAP version 14\n1..1\nnot ok 1 - path \\\\server \\#issue\n", &mut notifier);
        processor.ingest("Bail out! stop \\#now\n", &mut notifier);
        processor.finish(&mut notifier);

        assert!(notifier.events.iter().any(|event| event == "failure:path \\server #issue"));
        assert!(notifier.events.iter().any(|event| event == "bailout:stop #now"));
    }

    #[test]
    fn test_id_out_of_range_is_protocol_failure() {
        let input = "TAP version 14\n1..1\nok 2 - out\n";
        let (state, _notifier) = process_input(input);
        assert_eq!(state.protocol_failures, 1);
    }

    #[test]
    fn directive_suffixes_are_not_accepted() {
        let input = "TAP version 14\n1..2\nnot ok 1 - work # TODOfoo later\nnot ok 2 - skip # SKIPbar env\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.todo, 0);
        assert_eq!(state.skipped, 0);
        assert_eq!(state.failed, 2);
    }

    #[test]
    fn yaml_diagnostics_are_consumed_after_testpoint() {
        let input = "TAP version 14\n1..1\nnot ok 1 - oops\n  ---\n  severity: high\n  ...\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.parse_warning_count, 0);
        assert_eq!(state.protocol_failures, 0);
        assert_eq!(state.failed, 1);
    }

    #[test]
    fn unexpected_indented_line_warns_by_default() {
        let input = "TAP version 14\n1..1\n  invalid\nok 1 - done\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.parse_warning_count, 1);
        assert_eq!(state.protocol_failures, 0);
    }

    #[test]
    fn tap_version_line_after_content_is_protocol_failure() {
        let input = "# note\nTAP version 14\n1..1\nok 1 - done\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.protocol_failures, 1);
        assert!(!state.is_success());
    }

    #[test]
    fn nested_subtest_protocol_failures_propagate() {
        let input =
            "TAP version 14\n# Subtest: nested\n    ok 1 - missing plan\nnot ok 1 - nested\n1..1\n";
        let (state, _notifier) = process_input(input);

        assert!(state.protocol_failures >= 1);
        assert!(!state.is_success());
    }

    #[test]
    fn nested_subtest_parent_mismatch_is_protocol_failure() {
        let input = "TAP version 14\n# Subtest: nested\n    1..1\n    not ok 1 - child\nok 1 - nested\n1..1\n";
        let (state, _notifier) = process_input(input);

        assert_eq!(state.failed, 0);
        assert!(state.protocol_failures >= 1);
        assert!(!state.is_success());
    }

    #[test]
    fn nested_subtest_requires_parent_testpoint() {
        let input = "TAP version 14\n# Subtest: lonely\n    1..1\n    ok 1 - child\n1..0\n";
        let (state, _notifier) = process_input(input);

        assert!(state.protocol_failures >= 1);
        assert!(!state.is_success());
    }
}
