use std::io;
use std::time::SystemTime;

use anyhow::{Result, bail};
use clap::Parser;

use tapcue::cli::{Cli, CliCommand};
use tapcue::config::{EffectiveConfig, InputFormat};
use tapcue::notifier::{
    DesktopNotifier, NotificationPolicy, Notifier, NullNotifier, PolicyNotifier,
};
use tapcue::{AppConfig, process_stream};

mod doctor;
mod init;
mod junit_resolution;
mod run;
mod state;
mod summary;

use crate::doctor::{emit_doctor, emit_doctor_notifications};
use crate::init::run_init;
use crate::junit_resolution::{
    ingest_junit_reports, resolve_junit_report_files, should_prefer_inferred_junit,
};
use crate::run::{
    infer_junit_globs_for_command, resolve_run_command, run_and_process, run_and_wait,
};
use crate::state::{empty_run_state, merge_run_state};
use crate::summary::emit_summary;

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = &cli.command {
        match command {
            CliCommand::Init(init) => run_init(init)?,
            CliCommand::Doctor(doctor) => {
                let (effective_config, notification_sources) =
                    EffectiveConfig::load_with_sources(&Cli::without_overrides())?;
                if doctor.notifications {
                    emit_doctor_notifications(&effective_config);
                } else {
                    emit_doctor(&effective_config, &notification_sources);
                }
            }
            CliCommand::Run(_) => {}
        }
        if !matches!(command, CliCommand::Run(_)) {
            return Ok(());
        }
    }

    let effective_config = EffectiveConfig::load(&cli)?;

    if cli.print_effective_config {
        let rendered = effective_config.to_pretty_toml()?;
        print!("{rendered}");
        return Ok(());
    }

    if cli.validate_config {
        println!("tapcue: configuration is valid");
        return Ok(());
    }

    let mut base_notifier: Box<dyn Notifier> = if effective_config.no_notify {
        Box::new(NullNotifier)
    } else {
        Box::new(DesktopNotifier::new(
            effective_config.desktop_mode,
            resolve_project_label(&effective_config),
        ))
    };
    let mut notifier = PolicyNotifier::new(
        base_notifier.as_mut(),
        NotificationPolicy {
            dedup_failures: effective_config.dedup_failures,
            max_failure_notifications: effective_config.max_failure_notifications,
        },
    );

    let app_config = AppConfig {
        quiet_parse_errors: effective_config.quiet_parse_errors,
        strict: effective_config.strict,
        input_format: effective_config.input_format,
        trace_detection: effective_config.trace_detection,
    };

    let resolved_run_command = match &cli.command {
        Some(CliCommand::Run(run_cli)) => {
            Some(resolve_run_command(run_cli, &effective_config, SystemTime::now())?)
        }
        _ => None,
    };

    let inferred_runner_for_run = if effective_config.auto_junit_reports
        && effective_config.junit_file.is_empty()
        && effective_config.junit_dir.is_empty()
        && effective_config.junit_glob.is_empty()
    {
        resolved_run_command.as_ref().and_then(|resolved| resolved.inferred_runner).or_else(|| {
            resolved_run_command
                .as_ref()
                .and_then(|resolved| infer_junit_globs_for_command(&resolved.command).0)
        })
    } else {
        None
    };

    let skip_stream_parse_for_inferred_junit = inferred_runner_for_run.is_some()
        && !effective_config.junit_only
        && matches!(effective_config.input_format, InputFormat::Auto);

    let (mut state, child_status, run_started_at, summary_notified_by_stream) = match &cli.command {
        Some(CliCommand::Run(_run_cli)) => {
            let started_at = resolved_run_command
                .as_ref()
                .map(|resolved| resolved.started_at)
                .unwrap_or_else(SystemTime::now);
            let resolved = resolved_run_command
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("tapcue: missing resolved run command"))?;
            let (state, status, summary_notified_by_stream) =
                if effective_config.junit_only || skip_stream_parse_for_inferred_junit {
                    let status = run_and_wait(resolved)?;
                    (empty_run_state(), status, false)
                } else {
                    let (state, status) = run_and_process(
                        resolved,
                        &mut notifier,
                        app_config,
                        effective_config.run_output,
                    )?;
                    (state, status, true)
                };
            (state, Some(status), Some(started_at), summary_notified_by_stream)
        }
        _ => {
            let state = if effective_config.junit_only {
                empty_run_state()
            } else {
                process_stream(io::stdin().lock(), &mut notifier, app_config)?
            };
            (state, None, None, !effective_config.junit_only)
        }
    };

    let run_command = resolved_run_command.as_ref().map(|resolved| resolved.command.as_slice());

    let junit_reports = resolve_junit_report_files(
        &effective_config,
        run_command,
        resolved_run_command
            .as_ref()
            .map(|resolved| resolved.inferred_junit_files.as_slice())
            .unwrap_or(&[]),
        run_started_at,
        effective_config.trace_detection,
    )?;
    if effective_config.junit_only
        && junit_reports.files.is_empty()
        && !junit_reports.matched_existing_but_unmodified
    {
        bail!("tapcue: --junit-only requires at least one JUnit report input");
    }
    let junit_state = ingest_junit_reports(
        &junit_reports.files,
        &mut notifier,
        effective_config.quiet_parse_errors,
        effective_config.trace_detection,
    );

    if should_prefer_inferred_junit(&state, &junit_state, &junit_reports) {
        state = empty_run_state();
    }

    merge_run_state(&mut state, &junit_state);

    let suppress_summary = !summary_notified_by_stream
        && junit_reports.matched_existing_but_unmodified
        && junit_state.total == 0;
    if !suppress_summary {
        if !summary_notified_by_stream {
            notifier.notify_summary(&state);
        }
        emit_summary(&effective_config, &state)?;
    }

    if let Some(status) = child_status {
        if let Some(code) = status.code() {
            if code != 0 {
                std::process::exit(code);
            }
        } else {
            std::process::exit(1);
        }
    }

    if state.is_success() { Ok(()) } else { std::process::exit(1) }
}

fn resolve_project_label(config: &EffectiveConfig) -> Option<String> {
    if !config.include_project_context {
        return None;
    }

    if let Some(label) = config.project_label.as_ref() {
        let trimmed = label.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    let cwd = std::env::current_dir().ok()?;
    let basename = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned);

    basename.or_else(|| Some(cwd.display().to_string()))
}
