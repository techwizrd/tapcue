use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use clap::Parser;

use tapcue::cli::Cli;
use tapcue::config::{EffectiveConfig, SummaryFormat};
use tapcue::notifier::{
    DesktopNotifier, NotificationPolicy, Notifier, NullNotifier, PolicyNotifier,
};
use tapcue::processor::RunState;
use tapcue::{process_stream, AppConfig};

fn main() -> Result<()> {
    let cli = Cli::parse();
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
        Box::new(DesktopNotifier::new(effective_config.desktop_mode))
    };
    let mut notifier = PolicyNotifier::new(
        base_notifier.as_mut(),
        NotificationPolicy {
            dedup_failures: effective_config.dedup_failures,
            max_failure_notifications: effective_config.max_failure_notifications,
        },
    );

    let state = process_stream(
        io::stdin().lock(),
        &mut notifier,
        AppConfig {
            quiet_parse_errors: effective_config.quiet_parse_errors,
            input_format: effective_config.input_format,
            trace_detection: effective_config.trace_detection,
        },
    )?;

    emit_summary(&effective_config, &state)?;

    if state.is_success() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

fn emit_summary(config: &EffectiveConfig, state: &RunState) -> Result<()> {
    let rendered = match config.summary_format {
        SummaryFormat::None => None,
        SummaryFormat::Text => Some(render_text_summary(state)),
        SummaryFormat::Json => Some(render_json_summary(state)?),
    };

    if let Some(summary) = rendered {
        let summary_line = format!("{summary}\n");
        match summary_destination(config.summary_file.as_deref()) {
            SummaryDestination::Stdout => {
                io::stdout().write_all(summary_line.as_bytes())?;
            }
            SummaryDestination::File(path) => {
                fs::write(path, summary_line)?;
            }
        }
    }

    Ok(())
}

fn render_text_summary(state: &RunState) -> String {
    let status = if state.is_success() { "success" } else { "failure" };
    format!(
        "status={status} total={} passed={} failed={} todo={} skipped={} parse_warnings={}",
        state.total,
        state.passed,
        state.failed,
        state.todo,
        state.skipped,
        state.parse_warning_count,
    )
}

fn render_json_summary(state: &RunState) -> Result<String> {
    Ok(serde_json::to_string_pretty(state)?)
}

enum SummaryDestination<'a> {
    Stdout,
    File(&'a Path),
}

fn summary_destination(path: Option<&Path>) -> SummaryDestination<'_> {
    match path {
        None => SummaryDestination::Stdout,
        Some(raw_path) => match raw_path.to_string_lossy().as_ref() {
            "-" => SummaryDestination::Stdout,
            _ => SummaryDestination::File(raw_path),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{emit_summary, render_json_summary, render_text_summary, summary_destination};
    use tapcue::config::{DesktopMode, EffectiveConfig, InputFormat, SummaryFormat};
    use tapcue::processor::RunState;

    fn sample_state() -> RunState {
        RunState {
            planned: Some(2),
            total: 2,
            passed: 1,
            failed: 1,
            todo: 0,
            skipped: 0,
            bailout_reason: None,
            parse_warning_count: 0,
            protocol_failures: 0,
        }
    }

    fn sample_config() -> EffectiveConfig {
        EffectiveConfig {
            quiet_parse_errors: false,
            no_notify: true,
            desktop_mode: DesktopMode::Auto,
            input_format: InputFormat::Tap,
            summary_format: SummaryFormat::None,
            summary_file: None,
            dedup_failures: true,
            max_failure_notifications: None,
            trace_detection: false,
        }
    }

    #[test]
    fn render_text_summary_formats_expected_fields() {
        let rendered = render_text_summary(&sample_state());
        assert!(rendered.contains("status=failure"));
        assert!(rendered.contains("total=2"));
        assert!(rendered.contains("failed=1"));
    }

    #[test]
    fn render_json_summary_contains_json_fields() {
        let rendered = render_json_summary(&sample_state()).expect("json summary should render");
        assert!(rendered.contains("\"total\": 2"));
        assert!(rendered.contains("\"failed\": 1"));
    }

    #[test]
    fn summary_destination_dash_means_stdout() {
        let destination = summary_destination(Some(std::path::Path::new("-")));
        assert!(matches!(destination, super::SummaryDestination::Stdout));
    }

    #[test]
    fn emit_summary_writes_file_when_configured() {
        let dir = tempdir().expect("temp dir should create");
        let path = dir.path().join("summary.txt");
        let mut cfg = sample_config();
        cfg.summary_format = SummaryFormat::Text;
        cfg.summary_file = Some(path.clone());

        emit_summary(&cfg, &sample_state()).expect("emit summary should succeed");
        let content = fs::read_to_string(path).expect("summary file should exist");
        assert!(content.contains("status=failure"));
    }
}
