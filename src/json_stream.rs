use serde_json::Value;

use crate::line_buffer::LineBuffer;
use crate::notifier::{FailureNotification, FailureSource, Notifier};
use crate::processor::RunState;

#[derive(Debug)]
pub struct JsonStreamProcessor {
    quiet_parse_errors: bool,
    seen_json_line: bool,
    warned_parse_issue: bool,
    observed_test_events: bool,
    document_buffer: String,
    partial_line: LineBuffer,
    state: RunState,
}

impl JsonStreamProcessor {
    pub fn new(quiet_parse_errors: bool) -> Self {
        Self {
            quiet_parse_errors,
            seen_json_line: false,
            warned_parse_issue: false,
            observed_test_events: false,
            document_buffer: String::new(),
            partial_line: LineBuffer::default(),
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
        if !self.seen_json_line {
            self.document_buffer.push_str(chunk);
        }

        self.partial_line.push_str(chunk);

        while let Some(line) = self.partial_line.take_next_line() {
            self.process_line(&line, notifier);
        }
    }

    pub fn finish(&mut self, notifier: &mut dyn Notifier) {
        if let Some(line) = self.partial_line.take_remainder() {
            self.process_line(&line, notifier);
        }

        if !self.seen_json_line {
            let trimmed = self.document_buffer.trim();
            if !trimmed.is_empty() {
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(value) => self.apply_value(&value, notifier),
                    Err(_) => self.warn_once("unable to parse JSON stream", true),
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
        if trimmed.is_empty() {
            return;
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => {
                self.seen_json_line = true;
                self.document_buffer.clear();
                self.apply_value(&value, notifier);
            }
            Err(_) if self.seen_json_line => {
                self.warn_once("skipping non-JSON line in JSON stream", true);
            }
            Err(_) => {}
        }
    }

    fn apply_value(&mut self, value: &Value, notifier: &mut dyn Notifier) {
        if self.try_apply_go_event(value, notifier) {
            return;
        }

        if self.try_apply_nextest_event(value, notifier) {
            return;
        }

        self.try_apply_summary_document(value, notifier);
    }

    fn try_apply_go_event(&mut self, value: &Value, notifier: &mut dyn Notifier) -> bool {
        let Some(action) = value.get("Action").and_then(Value::as_str) else {
            return false;
        };

        let has_test = value.get("Test").and_then(Value::as_str).is_some();
        let label = value
            .get("Test")
            .and_then(Value::as_str)
            .or_else(|| value.get("Package").and_then(Value::as_str))
            .unwrap_or("unknown");
        let suite = value.get("Package").and_then(Value::as_str);
        let reason = value.get("Output").and_then(Value::as_str);

        match action {
            "pass" => {
                if has_test {
                    self.record_pass();
                }
            }
            "fail" => {
                if has_test {
                    self.record_failure(FailureSource::Go, label, suite, None, reason, notifier);
                }
            }
            "skip" => {
                if has_test {
                    self.record_skip();
                }
            }
            "output" | "run" => {}
            _ => {}
        }

        true
    }

    fn try_apply_nextest_event(&mut self, value: &Value, notifier: &mut dyn Notifier) -> bool {
        let Some(kind) = value.get("type").and_then(Value::as_str) else {
            return false;
        };

        if kind != "test" {
            return false;
        }

        let event_name = nextest_event_name(value);
        let label = value
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| value.get("test").and_then(Value::as_str))
            .unwrap_or("unknown");
        let suite = value
            .get("binary_id")
            .and_then(Value::as_str)
            .or_else(|| value.get("package").and_then(Value::as_str));
        let test_file = value.get("test_binary").and_then(Value::as_str);
        let reason = value
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| value.get("error").and_then(Value::as_str))
            .or_else(|| {
                value
                    .get("event")
                    .and_then(Value::as_object)
                    .and_then(|event| event.get("message"))
                    .and_then(Value::as_str)
            });

        match event_name {
            "passed" | "ok" => self.record_pass(),
            "failed" => self.record_failure(
                FailureSource::Nextest,
                label,
                suite,
                test_file,
                reason,
                notifier,
            ),
            "ignored" | "skipped" => self.record_skip(),
            "todo" => self.record_todo(),
            _ => {}
        }

        true
    }

    fn try_apply_summary_document(&mut self, value: &Value, notifier: &mut dyn Notifier) -> bool {
        if self.observed_test_events {
            return false;
        }

        if let Some(object) = value.as_object() {
            let total = number_field(object.get("numTotalTests"));
            let passed = number_field(object.get("numPassedTests"));
            let failed = number_field(object.get("numFailedTests"));
            let pending = number_field(object.get("numPendingTests"));
            let todo = number_field(object.get("numTodoTests"));

            if total.is_some() || passed.is_some() || failed.is_some() {
                self.state.total = total.unwrap_or(0);
                self.state.passed = passed.unwrap_or(0);
                self.state.failed = failed.unwrap_or(0);
                self.state.skipped = pending.unwrap_or(0);
                self.state.todo = todo.unwrap_or(0);

                self.collect_failed_labels(
                    object.get("testResults"),
                    FailureSource::Jest,
                    notifier,
                );
                return true;
            }

            if let Some(stats) = object.get("stats") {
                let total = number_field(stats.get("tests"));
                let passed = number_field(stats.get("passes"));
                let failed = number_field(stats.get("failures"));
                let skipped = number_field(stats.get("skipped"));
                if total.is_some() || passed.is_some() || failed.is_some() {
                    self.state.total = total.unwrap_or(0);
                    self.state.passed = passed.unwrap_or(0);
                    self.state.failed = failed.unwrap_or(0);
                    self.state.skipped = skipped.unwrap_or(0);
                    self.collect_failed_labels(
                        object.get("testResults"),
                        FailureSource::Vitest,
                        notifier,
                    );
                    return true;
                }
            }
        }

        false
    }

    fn collect_failed_labels(
        &mut self,
        test_results: Option<&Value>,
        source: FailureSource,
        notifier: &mut dyn Notifier,
    ) {
        let Some(entries) = test_results.and_then(Value::as_array) else {
            return;
        };

        for entry in entries {
            let suite_name = entry
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| entry.get("title").and_then(Value::as_str));

            if let Some(assertions) = entry.get("assertionResults").and_then(Value::as_array) {
                for assertion in assertions {
                    if assertion.get("status").and_then(Value::as_str) == Some("failed") {
                        let label = assertion
                            .get("fullName")
                            .and_then(Value::as_str)
                            .or_else(|| assertion.get("title").and_then(Value::as_str))
                            .unwrap_or("failed assertion");
                        let reason = first_non_empty_string(&[
                            assertion
                                .get("failureMessages")
                                .and_then(Value::as_array)
                                .and_then(|messages| messages.first())
                                .and_then(Value::as_str),
                            assertion.get("failureMessage").and_then(Value::as_str),
                            entry.get("message").and_then(Value::as_str),
                            Some(label),
                        ]);
                        let mut failure = FailureNotification::new(source, label);
                        failure.suite = suite_name.map(str::to_owned);
                        failure.test_file =
                            entry.get("name").and_then(Value::as_str).map(str::to_owned);
                        failure.reason = reason.map(short_reason);
                        notifier.notify_failure(&failure);
                    }
                }
            } else if entry.get("status").and_then(Value::as_str) == Some("failed") {
                let label = entry
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| entry.get("title").and_then(Value::as_str))
                    .unwrap_or("failed test suite");
                let reason = first_non_empty_string(&[
                    entry.get("message").and_then(Value::as_str),
                    entry.get("failureMessage").and_then(Value::as_str),
                    Some(label),
                ]);
                let mut failure = FailureNotification::new(source, label);
                failure.suite = suite_name.map(str::to_owned);
                failure.test_file = entry.get("name").and_then(Value::as_str).map(str::to_owned);
                failure.reason = reason.map(short_reason);
                notifier.notify_failure(&failure);
            }
        }
    }

    fn record_pass(&mut self) {
        self.observed_test_events = true;
        self.state.total += 1;
        self.state.passed += 1;
    }

    fn record_failure(
        &mut self,
        source: FailureSource,
        label: &str,
        suite: Option<&str>,
        test_file: Option<&str>,
        reason: Option<&str>,
        notifier: &mut dyn Notifier,
    ) {
        self.observed_test_events = true;
        self.state.total += 1;
        self.state.failed += 1;
        let mut failure = FailureNotification::new(source, label);
        failure.suite = suite.map(str::to_owned);
        failure.test_file = test_file.map(str::to_owned);
        failure.reason = Some(short_reason(reason.unwrap_or(label)));
        notifier.notify_failure(&failure);
    }

    fn record_skip(&mut self) {
        self.observed_test_events = true;
        self.state.total += 1;
        self.state.skipped += 1;
    }

    fn record_todo(&mut self) {
        self.observed_test_events = true;
        self.state.total += 1;
        self.state.todo += 1;
    }

    fn warn_once(&mut self, message: &str, emit_if_not_quiet: bool) {
        if self.warned_parse_issue {
            return;
        }

        self.warned_parse_issue = true;
        self.state.parse_warning_count += 1;
        if emit_if_not_quiet && !self.quiet_parse_errors {
            eprintln!("tapcue: parse warning: {message}");
        }
    }
}

fn number_field(value: Option<&Value>) -> Option<usize> {
    value.and_then(Value::as_u64).and_then(|raw| usize::try_from(raw).ok())
}

fn nextest_event_name(value: &Value) -> &str {
    let Some(event) = value.get("event") else {
        return "unknown";
    };

    if let Some(name) = event.as_str() {
        return name;
    }

    if let Some(name) = event.get("status").and_then(Value::as_str) {
        return name;
    }

    if let Some(name) = event.get("kind").and_then(Value::as_str) {
        return name;
    }

    "unknown"
}

fn first_non_empty_string<'a>(candidates: &[Option<&'a str>]) -> Option<&'a str> {
    candidates.iter().flatten().find(|value| !value.trim().is_empty()).copied()
}

fn short_reason(raw: &str) -> String {
    const LIMIT: usize = 160;

    let first_line = raw.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("unknown");

    let mut shortened = String::new();
    for (index, ch) in first_line.chars().enumerate() {
        if index >= LIMIT {
            shortened.push_str("...");
            return shortened;
        }
        shortened.push(ch);
    }

    shortened
}

#[cfg(test)]
mod tests {
    use crate::notifier::{FailureNotification, Notifier};

    use super::JsonStreamProcessor;
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
    fn parses_go_test_json_stream() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();
        processor.ingest("{\"Action\":\"pass\",\"Test\":\"TestA\"}\n", &mut notifier);
        processor.ingest("{\"Action\":\"fail\",\"Test\":\"TestB\"}\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures[0].label, "TestB");
        assert_eq!(notifier.failures[0].source.as_str(), "go");
    }

    #[test]
    fn parses_multiline_summary_json_document() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();
        let input =
            "{\n  \"numTotalTests\": 2,\n  \"numPassedTests\": 1,\n  \"numFailedTests\": 1\n}\n";
        processor.ingest(input, &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);
    }

    #[test]
    fn permissive_non_json_lines_do_not_fail_processing() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();
        processor.ingest("this is noise\n", &mut notifier);
        processor.ingest("{\"Action\":\"pass\",\"Test\":\"A\"}\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.passed, 1);
    }

    #[test]
    fn go_package_level_fail_does_not_count_as_test_failure() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("{\"Action\":\"fail\",\"Package\":\"example\"}\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 0);
        assert_eq!(state.failed, 0);
        assert!(notifier.failures.is_empty());
    }

    #[test]
    fn summary_document_ignored_after_test_events() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("{\"Action\":\"pass\",\"Test\":\"A\"}\n", &mut notifier);
        processor.ingest(
            "{\"numTotalTests\":5,\"numPassedTests\":0,\"numFailedTests\":5}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.passed, 1);
        assert_eq!(state.failed, 0);
    }

    #[test]
    fn nextest_object_event_with_status_is_supported() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(
            "{\"type\":\"test\",\"event\":{\"kind\":\"finished\",\"status\":\"failed\"},\"name\":\"crate::failing\"}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures[0].label, "crate::failing");
        assert_eq!(notifier.failures[0].source.as_str(), "nextest");
    }

    #[test]
    fn malformed_document_counts_parse_warning() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("{\"numTotalTests\":", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.parse_warning_count, 1);
        assert_eq!(notifier.summaries, 1);
    }

    #[test]
    fn vitest_stats_summary_document_is_applied() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(
            "{\"stats\":{\"tests\":4,\"passes\":2,\"failures\":1,\"skipped\":1}}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 4);
        assert_eq!(state.passed, 2);
        assert_eq!(state.failed, 1);
        assert_eq!(state.skipped, 1);
    }

    #[test]
    fn failed_suite_status_emits_failure_label() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(
            "{\"numTotalTests\":1,\"numPassedTests\":0,\"numFailedTests\":1,\"testResults\":[{\"status\":\"failed\",\"name\":\"suite-name\"}]}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures[0].label, "suite-name");
        assert_eq!(notifier.failures[0].source.as_str(), "jest");
    }

    #[test]
    fn empty_lines_and_non_json_after_json_emit_single_warning() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("\n", &mut notifier);
        processor.ingest("{\"Action\":\"pass\",\"Test\":\"A\"}\n", &mut notifier);
        processor.ingest("not-json\n", &mut notifier);
        processor.ingest("still-not-json\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.passed, 1);
        assert_eq!(state.parse_warning_count, 1);
    }

    #[test]
    fn nextest_string_events_for_todo_and_skip_are_supported() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(
            "{\"type\":\"test\",\"event\":\"todo\",\"name\":\"crate::todo\"}\n",
            &mut notifier,
        );
        processor.ingest(
            "{\"type\":\"test\",\"event\":\"skipped\",\"name\":\"crate::skip\"}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 2);
        assert_eq!(state.todo, 1);
        assert_eq!(state.skipped, 1);
    }

    #[test]
    fn jest_assertion_results_emit_failure_labels() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest(
            "{\"numTotalTests\":2,\"numPassedTests\":1,\"numFailedTests\":1,\"testResults\":[{\"assertionResults\":[{\"status\":\"failed\",\"fullName\":\"suite should fail\"}]}]}\n",
            &mut notifier,
        );
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures[0].label, "suite should fail");
        assert_eq!(notifier.failures[0].source.as_str(), "jest");
    }

    #[test]
    fn nextest_ok_event_counts_pass() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor
            .ingest("{\"type\":\"test\",\"event\":\"ok\",\"name\":\"crate::ok\"}\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 1);
        assert_eq!(state.passed, 1);
    }

    #[test]
    fn go_output_and_run_events_do_not_affect_counts() {
        let mut processor = JsonStreamProcessor::new(false);
        let mut notifier = RecordingNotifier::default();

        processor.ingest("{\"Action\":\"run\",\"Test\":\"A\"}\n", &mut notifier);
        processor.ingest("{\"Action\":\"output\",\"Output\":\"hello\"}\n", &mut notifier);
        processor.finish(&mut notifier);

        let state = processor.into_state();
        assert_eq!(state.total, 0);
        assert_eq!(state.passed, 0);
        assert_eq!(state.failed, 0);
    }
}
