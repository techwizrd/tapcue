use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use tapcue::config::EffectiveConfig;
use tapcue::junit_reports::ingest_junit_file;
use tapcue::notifier::Notifier;
use tapcue::processor::RunState;

use crate::run::{InferredJunitRunner, infer_junit_globs_for_command};
use crate::state::{empty_run_state, merge_run_state};

pub(crate) struct JunitReportResolution {
    pub(crate) files: Vec<PathBuf>,
    pub(crate) inferred_runner: Option<InferredJunitRunner>,
    pub(crate) matched_existing_but_unmodified: bool,
}

pub(crate) fn resolve_junit_report_files(
    config: &EffectiveConfig,
    run_command: Option<&[String]>,
    inferred_junit_files: &[PathBuf],
    run_started_at: Option<SystemTime>,
    trace_detection: bool,
) -> Result<JunitReportResolution> {
    let mut files = collect_configured_junit_files(config)?;
    files.extend_from_slice(inferred_junit_files);

    let has_explicit_inputs = !config.junit_file.is_empty()
        || !config.junit_dir.is_empty()
        || !config.junit_glob.is_empty();

    let mut inferred_runner = None;
    if files.is_empty() && config.auto_junit_reports {
        if let Some(run_cli) = run_command {
            let (runner, inferred_globs) = infer_junit_globs_for_command(run_cli);
            inferred_runner = runner;
            if trace_detection && !inferred_globs.is_empty() {
                eprintln!("tapcue: inferred JUnit report globs: {}", inferred_globs.join(", "));
            }

            for pattern in inferred_globs {
                collect_glob_matches(&pattern, &mut files)?;
            }
        }
    }

    let mut matched_existing_but_unmodified = false;
    if let Some(started_at) = run_started_at {
        let apply_freshness_filter = inferred_runner.is_some() || has_explicit_inputs;
        if apply_freshness_filter {
            let pre_filter_count = files.len();
            files.retain(|path| {
                fs::metadata(path)
                    .and_then(|metadata| metadata.modified())
                    .map(|modified| is_fresh_report(modified, started_at))
                    .unwrap_or(false)
            });

            if files.is_empty() && pre_filter_count > 0 {
                matched_existing_but_unmodified = true;
                if trace_detection {
                    eprintln!("tapcue: JUnit reports exist but none were modified in this run");
                }
            }
        }
    }

    files.sort();
    files.dedup();
    Ok(JunitReportResolution { files, inferred_runner, matched_existing_but_unmodified })
}

fn collect_configured_junit_files(config: &EffectiveConfig) -> Result<Vec<PathBuf>> {
    let mut files = config.junit_file.clone();

    for dir_path in &config.junit_dir {
        let pattern = format!("{}/**/*.xml", dir_path.display());
        collect_glob_matches(&pattern, &mut files).map_err(|error| {
            anyhow::anyhow!(
                "tapcue: invalid JUnit directory expansion for {dir}: {error}",
                dir = dir_path.display()
            )
        })?;
    }

    for pattern in &config.junit_glob {
        collect_glob_matches(pattern, &mut files).map_err(|error| {
            anyhow::anyhow!("tapcue: invalid JUnit glob match for {pattern}: {error}")
        })?;
    }

    Ok(files)
}

fn collect_glob_matches(pattern: &str, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in glob::glob(pattern)? {
        match entry {
            Ok(path) if path.is_file() => files.push(path),
            Ok(_) => {}
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        }
    }

    Ok(())
}

fn is_fresh_report(modified: SystemTime, started_at: SystemTime) -> bool {
    const MTIME_TOLERANCE: Duration = Duration::from_secs(2);
    match started_at.checked_sub(MTIME_TOLERANCE) {
        Some(threshold) => modified >= threshold,
        None => true,
    }
}

pub(crate) fn should_prefer_inferred_junit(
    stream_state: &RunState,
    junit_state: &RunState,
    junit_reports: &JunitReportResolution,
) -> bool {
    junit_reports.inferred_runner.is_some()
        && junit_state.total > 0
        && stream_state.total == 0
        && stream_state.failed == 0
        && stream_state.protocol_failures > 0
}

pub(crate) fn ingest_junit_reports(
    junit_files: &[PathBuf],
    notifier: &mut dyn Notifier,
    quiet_parse_errors: bool,
    trace_detection: bool,
) -> RunState {
    let mut state = empty_run_state();

    for file in junit_files {
        if trace_detection {
            eprintln!("tapcue: ingesting JUnit XML report: {}", file.display());
        }

        match ingest_junit_file(file, notifier) {
            Ok(parsed) => merge_run_state(&mut state, &parsed),
            Err(error) => {
                state.parse_warning_count += 1;
                if !quiet_parse_errors {
                    eprintln!("tapcue: parse warning: {error}");
                }
            }
        }
    }

    state
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::is_fresh_report;

    #[test]
    fn fresh_report_tolerates_small_mtime_skew() {
        let started = SystemTime::now();
        let modified = started - Duration::from_secs(1);
        assert!(is_fresh_report(modified, started));

        let old_modified = started - Duration::from_secs(5);
        assert!(!is_fresh_report(old_modified, started));
    }
}
