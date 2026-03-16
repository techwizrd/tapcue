use std::fs;
use std::path::Path;

use roxmltree::{Document, Node};

use crate::notifier::{FailureNotification, FailureSource, Notifier};
use crate::processor::RunState;

pub fn ingest_junit_file(path: &Path, notifier: &mut dyn Notifier) -> Result<RunState, String> {
    let source = fs::read_to_string(path)
        .map_err(|err| format!("failed to read JUnit XML file {}: {err}", path.display()))?;
    ingest_junit_xml_str(&source, notifier)
        .map_err(|err| format!("failed to parse JUnit XML file {}: {err}", path.display()))
}

pub fn ingest_junit_xml_str(
    source: &str,
    notifier: &mut dyn Notifier,
) -> Result<RunState, roxmltree::Error> {
    let document = Document::parse(source)?;
    let mut state = RunState {
        planned: None,
        total: 0,
        passed: 0,
        failed: 0,
        todo: 0,
        skipped: 0,
        bailout_reason: None,
        parse_warning_count: 0,
        protocol_failures: 0,
    };

    for testcase in document.descendants().filter(is_testcase) {
        state.total += 1;

        if has_child_element(testcase, "skipped") {
            state.skipped += 1;
            continue;
        }

        let failure_node = testcase.children().find(|node| {
            node.is_element()
                && (node.tag_name().name() == "failure" || node.tag_name().name() == "error")
        });

        if let Some(failure) = failure_node {
            state.failed += 1;

            let mut notification =
                FailureNotification::new(FailureSource::Junit, testcase_label(testcase));
            notification.suite = nearest_testsuite_name(testcase);
            notification.test_file = testcase
                .attribute("file")
                .map(str::to_owned)
                .or_else(|| nearest_testsuite_file(testcase));
            notification.reason = failure_reason(failure);
            notifier.notify_failure(&notification);
        } else {
            state.passed += 1;
        }
    }

    Ok(state)
}

fn is_testcase(node: &Node<'_, '_>) -> bool {
    node.is_element() && node.tag_name().name() == "testcase"
}

fn has_child_element(node: Node<'_, '_>, tag_name: &str) -> bool {
    node.children().any(|child| child.is_element() && child.tag_name().name() == tag_name)
}

fn testcase_label(testcase: Node<'_, '_>) -> String {
    let name = testcase.attribute("name").unwrap_or("unnamed testcase").trim();
    let class_name = testcase.attribute("classname").unwrap_or("").trim();

    if !class_name.is_empty() && !name.is_empty() {
        format!("{class_name}::{name}")
    } else if !name.is_empty() {
        name.to_owned()
    } else if !class_name.is_empty() {
        class_name.to_owned()
    } else {
        "unnamed testcase".to_owned()
    }
}

fn nearest_testsuite_name(node: Node<'_, '_>) -> Option<String> {
    node.ancestors().find_map(|ancestor| {
        if ancestor.is_element() && ancestor.tag_name().name() == "testsuite" {
            ancestor.attribute("name").map(str::to_owned)
        } else {
            None
        }
    })
}

fn nearest_testsuite_file(node: Node<'_, '_>) -> Option<String> {
    node.ancestors().find_map(|ancestor| {
        if ancestor.is_element() && ancestor.tag_name().name() == "testsuite" {
            ancestor.attribute("file").map(str::to_owned)
        } else {
            None
        }
    })
}

fn failure_reason(failure_node: Node<'_, '_>) -> Option<String> {
    if let Some(message) = failure_node.attribute("message") {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            return Some(first_line(trimmed));
        }
    }

    let text = failure_node.text().unwrap_or("").trim();
    if text.is_empty() {
        None
    } else {
        Some(first_line(text))
    }
}

fn first_line(raw: &str) -> String {
    raw.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or(raw).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::notifier::{FailureNotification, Notifier};

    use super::ingest_junit_xml_str;
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
    fn parses_junit_testsuite_with_failures_and_skips() {
        let xml = r#"
<testsuite name="suite-a">
  <testcase classname="math" name="adds" />
  <testcase classname="math" name="divides">
    <failure message="division by zero">stack line</failure>
  </testcase>
  <testcase classname="math" name="slow">
    <skipped />
  </testcase>
</testsuite>
"#;

        let mut notifier = RecordingNotifier::default();
        let state = ingest_junit_xml_str(xml, &mut notifier).expect("junit xml should parse");

        assert_eq!(state.total, 3);
        assert_eq!(state.passed, 1);
        assert_eq!(state.failed, 1);
        assert_eq!(state.skipped, 1);
        assert_eq!(notifier.failures.len(), 1);
        assert_eq!(notifier.failures[0].label, "math::divides");
        assert_eq!(notifier.failures[0].suite.as_deref(), Some("suite-a"));
        assert_eq!(notifier.failures[0].reason.as_deref(), Some("division by zero"));
    }

    #[test]
    fn parses_junit_testsuites_root() {
        let xml = r#"
<testsuites>
  <testsuite name="suite-one">
    <testcase classname="a" name="ok" />
  </testsuite>
  <testsuite name="suite-two">
    <testcase classname="b" name="err">
      <error>boom</error>
    </testcase>
  </testsuite>
</testsuites>
"#;

        let mut notifier = RecordingNotifier::default();
        let state = ingest_junit_xml_str(xml, &mut notifier).expect("junit xml should parse");

        assert_eq!(state.total, 2);
        assert_eq!(state.passed, 1);
        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures.len(), 1);
        assert_eq!(notifier.failures[0].suite.as_deref(), Some("suite-two"));
    }
}
