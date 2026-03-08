use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;

fn fixture(name: &str) -> String {
    let path = format!("tests/fixtures/{name}");
    fs::read_to_string(path).expect("fixture should load")
}

#[test]
fn auto_detects_go_test_json_stream() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("go_test_json.ndjson"))
        .assert()
        .failure();
}

#[test]
fn auto_detects_nextest_json_stream() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("nextest_json.ndjson"))
        .assert()
        .failure();
}

#[test]
fn auto_detects_jest_json_report() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("jest_report.json"))
        .assert()
        .failure();
}

#[test]
fn auto_detects_vitest_json_report() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("vitest_report.json"))
        .assert()
        .failure();
}

#[test]
fn auto_detects_tap_output_from_npm_test() {
    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .write_stdin(fixture("npm_tap.tap"))
        .assert()
        .failure();
}

#[test]
fn forced_json_mode_is_permissive_with_noise_lines() {
    let input = "npm notice something\n{\"Action\":\"pass\",\"Test\":\"Alpha\"}\n";

    Command::new(env!("CARGO_BIN_EXE_tapcue"))
        .arg("--no-notify")
        .arg("--format")
        .arg("json")
        .write_stdin(input)
        .assert()
        .success()
        .stderr(predicate::str::contains("parse warning").or(predicate::str::is_empty()));
}
