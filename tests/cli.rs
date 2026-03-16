use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    fs::read_to_string(path).expect("fixture should load")
}

#[test]
fn cli_returns_zero_for_passing_input() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("passing.tap"))
        .assert()
        .success();
}

#[test]
fn cli_returns_non_zero_for_failing_input() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("failing.tap"))
        .assert()
        .failure();
}

#[test]
fn strict_flag_enforces_tap14_failures_without_pragma() {
    let input = "TAP version 14\n1..1\nthis is invalid\nok 1 - valid test\n";

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(input)
        .assert()
        .success();

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--strict")
        .write_stdin(input)
        .assert()
        .failure();
}

#[test]
fn print_effective_config_outputs_merged_values() {
    let dir = tempdir().expect("temp dir should create");
    let local_cfg = dir.path().join(".tapcue.toml");
    fs::write(
        local_cfg,
        "[parser]\nquiet_parse_errors = true\n[notifications]\nenabled = false\ndesktop = \"force-off\"\n",
    )
    .expect("local config should write");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--print-effective-config")
        .arg("--desktop")
        .arg("force-on")
        .current_dir(dir.path())
        .env("TAPCUE_NO_NOTIFY", "false")
        .assert()
        .success()
        .stdout(predicates::str::contains("[parser]"))
        .stdout(predicates::str::contains("quiet_parse_errors = true"))
        .stdout(predicates::str::contains("[input]"))
        .stdout(predicates::str::contains("format = \"auto\""))
        .stdout(predicates::str::contains("[notifications]"))
        .stdout(predicates::str::contains("enabled = true"))
        .stdout(predicates::str::contains("desktop = \"force-on\""));
}

#[test]
fn cli_force_flags_override_env_and_local_values() {
    let dir = tempdir().expect("temp dir should create");
    fs::write(
        dir.path().join(".tapcue.toml"),
        "[parser]\nquiet_parse_errors = true\n[notifications]\nenabled = false\n",
    )
    .expect("local config should write");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--print-effective-config")
        .arg("--notify")
        .arg("--no-quiet-parse-errors")
        .current_dir(dir.path())
        .env("TAPCUE_NO_NOTIFY", "true")
        .env("TAPCUE_QUIET_PARSE_ERRORS", "true")
        .assert()
        .success()
        .stdout(predicates::str::contains("quiet_parse_errors = false"))
        .stdout(predicates::str::contains("enabled = true"));
}

#[test]
fn validate_config_exits_success_without_input() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--validate-config")
        .assert()
        .success()
        .stdout(predicates::str::contains("configuration is valid"));
}

#[test]
fn init_writes_default_config_file() {
    let dir = tempdir().expect("temp dir should create");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("wrote .tapcue.toml"));

    let content =
        fs::read_to_string(dir.path().join(".tapcue.toml")).expect("config file should exist");
    assert!(content.contains("[parser]"));
    assert!(content.contains("quiet_parse_errors = false"));
    assert!(content.contains("[notifications]"));
    assert!(content.contains("enabled = true"));
}

#[test]
fn init_current_writes_effective_config() {
    let dir = tempdir().expect("temp dir should create");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("init")
        .arg("--current")
        .current_dir(dir.path())
        .env("TAPCUE_NO_NOTIFY", "true")
        .assert()
        .success();

    let content =
        fs::read_to_string(dir.path().join(".tapcue.toml")).expect("config file should exist");
    assert!(content.contains("enabled = false"));
}

#[test]
fn init_refuses_to_overwrite_without_force() {
    let dir = tempdir().expect("temp dir should create");
    fs::write(dir.path().join(".tapcue.toml"), "[parser]\nstrict = true\n")
        .expect("existing config should write");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("already exists"));
}

#[test]
fn init_force_overwrites_existing_file() {
    let dir = tempdir().expect("temp dir should create");
    let path = dir.path().join(".tapcue.toml");
    fs::write(&path, "invalid = true\n").expect("existing config should write");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("init")
        .arg("--force")
        .current_dir(dir.path())
        .assert()
        .success();

    let content = fs::read_to_string(path).expect("config file should exist");
    assert!(content.contains("[notifications]"));
    assert!(!content.contains("invalid = true"));
}

#[test]
fn summary_json_emits_to_file_destination() {
    let dir = tempdir().expect("temp dir should create");
    let summary_path = dir.path().join("summary.json");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("json")
        .arg("--summary-file")
        .arg(summary_path.to_string_lossy().to_string())
        .write_stdin(fixture("failing.tap"))
        .assert()
        .failure()
        .stdout(predicates::str::is_empty());

    let summary = fs::read_to_string(summary_path).expect("summary file should be written");
    assert!(summary.contains("\"total\": 2"));
    assert!(summary.contains("\"failed\": 1"));
}

#[test]
fn summary_file_dash_writes_to_stdout() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("json")
        .arg("--summary-file")
        .arg("-")
        .write_stdin(fixture("failing.tap"))
        .assert()
        .failure()
        .stdout(predicates::str::contains("\"failed\": 1"));
}

#[test]
fn summary_text_emits_text_line() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .write_stdin(fixture("passing.tap"))
        .assert()
        .success()
        .stdout(predicates::str::contains("status=success total=2 passed=2"));
}

#[test]
fn invalid_config_file_fails_with_context() {
    let dir = tempdir().expect("temp dir should create");
    fs::write(dir.path().join(".tapcue.toml"), "[notifications\nenabled=true\n")
        .expect("invalid file should be created");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .current_dir(dir.path())
        .arg("--validate-config")
        .assert()
        .failure()
        .stderr(predicates::str::contains("failed to parse TOML config file"));
}

#[test]
fn doctor_runs_without_input() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicates::str::contains("doctor:"))
        .stdout(predicates::str::contains("ready"))
        .stdout(predicates::str::contains("settings:"))
        .stdout(predicates::str::contains("notifications.enabled"));
}

#[test]
fn doctor_explains_environment_disabled_notifications() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("doctor")
        .env("TAPCUE_NO_NOTIFY", "true")
        .assert()
        .success()
        .stdout(predicates::str::contains("action needed"))
        .stdout(predicates::str::contains("notifications.enabled"))
        .stdout(predicates::str::contains("(source: environment)"))
        .stdout(predicates::str::contains("reasons:"))
        .stdout(predicates::str::contains("disabled by merged configuration"));
}

#[test]
fn doctor_flag_is_not_supported() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--doctor")
        .assert()
        .failure()
        .stderr(predicates::str::contains("unexpected argument '--doctor'"));
}

#[test]
fn junit_only_mode_fails_for_failing_report_file() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--junit-file")
        .arg("tests/fixtures/junit_failure.xml")
        .arg("--junit-only")
        .assert()
        .failure();
}

#[test]
fn junit_only_mode_succeeds_for_passing_report_file() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--junit-file")
        .arg("tests/fixtures/junit_success.xml")
        .arg("--junit-only")
        .assert()
        .success();
}

#[test]
fn junit_only_mode_emits_text_summary_for_fresh_report() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .arg("--summary-file")
        .arg("-")
        .arg("--junit-file")
        .arg("tests/fixtures/junit_success.xml")
        .arg("--junit-only")
        .assert()
        .success()
        .stdout(predicate::str::contains("status=success"))
        .stdout(predicate::str::contains("total=2"));
}

#[test]
fn junit_only_requires_report_inputs() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--junit-only")
        .assert()
        .failure()
        .stderr(predicates::str::contains("--junit-only requires at least one JUnit report input"));
}

#[cfg(unix)]
#[test]
fn run_mode_auto_discovers_gradle_junit_reports() {
    let dir = tempdir().expect("temp dir should create");
    let gradlew = dir.path().join("gradlew");
    let script = r#"#!/usr/bin/env sh
set -eu
mkdir -p build/test-results/test
cat > build/test-results/test/TEST-sample.xml <<'EOF'
<testsuite name="sample-suite">
  <testcase classname="sample.MathTest" name="ok" />
  <testcase classname="sample.MathTest" name="boom">
    <failure message="exploded" />
  </testcase>
</testsuite>
EOF
"#;
    fs::write(&gradlew, script).expect("gradlew script should write");
    let mut permissions =
        fs::metadata(&gradlew).expect("gradlew metadata should read").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gradlew, permissions).expect("gradlew permissions should set");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .arg("run")
        .arg("--")
        .arg("./gradlew")
        .arg("test")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stdout(predicates::str::contains("status=failure"));
}

#[cfg(unix)]
#[test]
fn run_mode_uses_existing_inferred_gradle_report_when_up_to_date() {
    let dir = tempdir().expect("temp dir should create");
    let report_dir = dir.path().join("build/test-results/test");
    fs::create_dir_all(&report_dir).expect("report directory should create");
    fs::write(
        report_dir.join("TEST-sample.xml"),
        "<testsuite name=\"sample-suite\"><testcase classname=\"sample\" name=\"ok\" /></testsuite>",
    )
    .expect("report should write");
    std::thread::sleep(Duration::from_secs(3));

    let gradlew = dir.path().join("gradlew");
    let script = r#"#!/usr/bin/env sh
set -eu
echo "> Task :app:testDebugUnitTest UP-TO-DATE"
echo "BUILD SUCCESSFUL in 1s"
"#;
    fs::write(&gradlew, script).expect("gradlew script should write");
    let mut permissions =
        fs::metadata(&gradlew).expect("gradlew metadata should read").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gradlew, permissions).expect("gradlew permissions should set");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .arg("run")
        .arg("--")
        .arg("./gradlew")
        .arg("testDebugUnitTest")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("status=").not());
}

#[cfg(unix)]
#[test]
fn junit_only_run_mode_ignores_stale_explicit_reports() {
    let dir = tempdir().expect("temp dir should create");
    let report_dir = dir.path().join("build/test-results/test");
    fs::create_dir_all(&report_dir).expect("report directory should create");
    fs::write(
        report_dir.join("TEST-sample.xml"),
        "<testsuite name=\"sample-suite\"><testcase classname=\"sample\" name=\"ok\" /></testsuite>",
    )
    .expect("report should write");
    std::thread::sleep(Duration::from_secs(3));

    let gradlew = dir.path().join("gradlew");
    let script = r#"#!/usr/bin/env sh
set -eu
echo "> Task :app:testDebugUnitTest UP-TO-DATE"
echo "BUILD SUCCESSFUL in 1s"
"#;
    fs::write(&gradlew, script).expect("gradlew script should write");
    let mut permissions =
        fs::metadata(&gradlew).expect("gradlew metadata should read").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gradlew, permissions).expect("gradlew permissions should set");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .arg("--junit-dir")
        .arg("build/test-results")
        .arg("--junit-only")
        .arg("run")
        .arg("--")
        .arg("./gradlew")
        .arg("testDebugUnitTest")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("status=").not());
}

#[cfg(unix)]
#[test]
fn run_mode_keeps_stream_summary_when_explicit_junit_is_stale() {
    let dir = tempdir().expect("temp dir should create");
    let report_dir = dir.path().join("build/test-results/test");
    fs::create_dir_all(&report_dir).expect("report directory should create");
    fs::write(
        report_dir.join("TEST-stale.xml"),
        "<testsuite name=\"stale\"><testcase classname=\"sample\" name=\"old\" /></testsuite>",
    )
    .expect("stale report should write");
    std::thread::sleep(Duration::from_secs(3));

    let runner = dir.path().join("runner.sh");
    let script = r#"#!/usr/bin/env sh
set -eu
printf 'TAP version 14\n1..1\nok 1 - fresh\n'
"#;
    fs::write(&runner, script).expect("runner script should write");
    let mut permissions = fs::metadata(&runner).expect("runner metadata should read").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&runner, permissions).expect("runner permissions should set");

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--summary-format")
        .arg("text")
        .arg("--summary-file")
        .arg("-")
        .arg("--junit-dir")
        .arg("build/test-results")
        .arg("run")
        .arg("--")
        .arg("./runner.sh")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("status=success"))
        .stdout(predicate::str::contains("total=1"));
}
