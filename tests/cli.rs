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
