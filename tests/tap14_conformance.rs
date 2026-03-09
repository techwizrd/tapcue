use assert_cmd::Command;
use predicates::str::contains;

fn run_tap(input: &str) -> assert_cmd::assert::Assert {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--format")
        .arg("tap")
        .arg("--summary-format")
        .arg("json")
        .write_stdin(input)
        .assert()
}

#[test]
fn tap13_version_is_accepted_for_compatibility() {
    run_tap("TAP version 13\n1..1\nok 1 - compat\n").success();
}

#[test]
fn missing_plan_fails() {
    run_tap("TAP version 14\nok 1 - missing plan\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn plan_must_match_test_count() {
    run_tap("TAP version 14\n1..2\nok 1 - only\n").failure().stdout(contains("\"planned\": 2"));
}

#[test]
fn plan_1_0_with_no_tests_passes() {
    run_tap("TAP version 14\n1..0 # skip all\n").success();
}

#[test]
fn strict_pragma_turns_invalid_lines_into_failures() {
    run_tap("TAP version 14\npragma +strict\n1..1\ninvalid stuff\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn invalid_lines_warn_but_are_non_fatal_by_default() {
    run_tap("TAP version 14\n1..1\ninvalid stuff\nok 1 - good\n")
        .success()
        .stdout(contains("\"parse_warning_count\": 1"));
}

#[test]
fn bailout_is_case_insensitive_and_fails_run() {
    run_tap("TAP version 14\n1..1\nBAIL OUT! stop now\n")
        .failure()
        .stdout(contains("\"bailout_reason\": \"stop now\""));
}

#[test]
fn todo_and_skip_directives_are_case_insensitive() {
    run_tap("TAP version 14\n1..2\nnot ok 1 - todo # ToDo later\nnot ok 2 - skipped # sKiP env\n")
        .success()
        .stdout(contains("\"todo\": 1"))
        .stdout(contains("\"skipped\": 1"));
}

#[test]
fn subtest_correlated_parent_testpoint_drives_outcome() {
    run_tap(
        "TAP version 14\n# Subtest: nested\n    1..2\n    ok 1 - child pass\n    not ok 2 - child fail\nnot ok 1 - nested\n1..1\n",
    )
    .failure()
    .stdout(contains("\"failed\": 1"));
}

#[test]
fn crlf_is_accepted() {
    run_tap("TAP version 14\r\n1..1\r\nok 1 - crlf\r\n").success();
}

#[test]
fn cr_only_line_endings_are_accepted() {
    run_tap("TAP version 14\r1..1\rok 1 - cr\r").success();
}

#[test]
fn duplicate_plan_is_protocol_failure() {
    run_tap("TAP version 14\n1..1\nok 1 - first\n1..1\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn testpoint_after_trailing_plan_fails() {
    run_tap("TAP version 14\nok 1 - first\n1..1\nok 2 - invalid\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn escaped_hash_does_not_create_directive() {
    run_tap("TAP version 14\n1..1\nnot ok 1 - failed \\# TODO not a directive\n")
        .failure()
        .stdout(contains("\"failed\": 1"))
        .stdout(contains("\"todo\": 0"));
}

#[test]
fn directive_suffixes_are_not_treated_as_valid_directives() {
    run_tap(
        "TAP version 14\n1..2\nnot ok 1 - work # TODOfoo later\nnot ok 2 - skip # SKIPbar env\n",
    )
    .failure()
    .stdout(contains("\"failed\": 2"))
    .stdout(contains("\"todo\": 0"))
    .stdout(contains("\"skipped\": 0"));
}

#[test]
fn out_of_range_test_id_fails() {
    run_tap("TAP version 14\n1..1\nok 2 - outside\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn duplicate_test_id_is_protocol_failure() {
    run_tap("TAP version 14\n1..2\nok 1 - first\nok 1 - duplicate\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn tap13_stream_with_trailing_plan_is_supported() {
    run_tap("TAP version 13\nok 1 - a\nok 2 - b\n1..2\n").success();
}

#[test]
fn yaml_diagnostics_after_testpoint_are_accepted() {
    run_tap("TAP version 14\n1..1\nnot ok 1 - oops\n  ---\n  severity: high\n  ...\n")
        .failure()
        .stdout(contains("\"parse_warning_count\": 0"));
}

#[test]
fn indented_non_subtest_line_warns_and_fails_in_strict_mode() {
    run_tap("TAP version 14\npragma +strict\n1..1\n  not-tap\nok 1 - done\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn version_line_must_be_first_non_empty_line_if_present() {
    run_tap("# comment before version\nTAP version 14\n1..1\nok 1 - test\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn nested_subtest_failure_with_ok_parent_is_protocol_failure() {
    run_tap(
        "TAP version 14\n# Subtest: nested\n    1..1\n    not ok 1 - child fail\nok 1 - nested\n1..1\n",
    )
    .failure()
    .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn strict_mode_can_be_disabled_with_pragma_minus_strict() {
    run_tap("TAP version 14\npragma +strict\npragma -strict\n1..1\ninvalid stuff\nok 1 - good\n")
        .success()
        .stdout(contains("\"protocol_failures\": 0"))
        .stdout(contains("\"parse_warning_count\": 1"));
}

#[test]
fn malformed_yaml_in_strict_mode_is_protocol_failure() {
    run_tap("TAP version 14\npragma +strict\n1..1\nnot ok 1 - bad\n  ---\nnot yaml body\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"))
        .stdout(contains("\"parse_warning_count\": 1"));
}

#[test]
fn nested_subtest_bailout_is_propagated_to_parent_run() {
    run_tap(
        "TAP version 14\n# Subtest: nested\n    1..1\n    Bail out! inner stop\nnot ok 1 - nested\n1..1\n",
    )
    .failure()
    .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn duplicate_tap_version_line_is_protocol_failure() {
    run_tap("TAP version 14\nTAP version 14\n1..1\nok 1 - once\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn tap_version_after_testpoint_is_protocol_failure() {
    run_tap("ok 1 - first\nTAP version 14\n1..1\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn comment_between_subtest_and_parent_testpoint_is_protocol_failure() {
    run_tap("TAP version 14\n# Subtest: nested\n    1..1\n    ok 1 - child\n# about nested\nok 1 - nested\n1..1\n")
        .failure()
        .stdout(contains("\"protocol_failures\": 1"));
}

#[test]
fn blank_line_between_subtest_and_parent_testpoint_is_accepted() {
    run_tap(
        "TAP version 14\n# Subtest: nested\n    1..1\n    ok 1 - child\n\nok 1 - nested\n1..1\n",
    )
    .success()
    .stdout(contains("\"protocol_failures\": 0"));
}
