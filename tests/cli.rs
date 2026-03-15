use std::fs;

use assert_cmd::Command;
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
