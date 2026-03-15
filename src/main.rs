use std::fs;
use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Result};
use clap::Parser;

use tapcue::cli::{Cli, CliCommand, InitCli};
use tapcue::config::{
    resolved_config_paths, EffectiveConfig, NotificationConfigSources, SummaryFormat,
};
use tapcue::notifier::{
    doctor_notifications, DesktopNotifier, NotificationPolicy, Notifier, NullNotifier,
    PolicyNotifier,
};
use tapcue::processor::RunState;
use tapcue::{process_stream, AppConfig};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = &cli.command {
        match command {
            CliCommand::Init(init) => run_init(init)?,
            CliCommand::Doctor => {
                let (effective_config, notification_sources) =
                    EffectiveConfig::load_with_sources(&Cli::without_overrides())?;
                emit_doctor(&effective_config, &notification_sources);
            }
        }
        return Ok(());
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
            strict: effective_config.strict,
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

fn run_init(init: &InitCli) -> Result<()> {
    let path = Path::new(".tapcue.toml");

    if path.exists() && !init.force {
        bail!(
            "tapcue: {} already exists; rerun with `tapcue init --force` to overwrite",
            path.display()
        );
    }

    let config = if init.current {
        EffectiveConfig::load(&Cli::without_overrides())?
    } else {
        EffectiveConfig::default()
    };

    let rendered = config.to_pretty_toml()?;
    fs::write(path, rendered)?;

    if init.current {
        println!("tapcue: wrote {} from current effective config", path.display());
    } else {
        println!("tapcue: wrote {} from built-in defaults", path.display());
    }

    Ok(())
}

fn emit_doctor(config: &EffectiveConfig, sources: &NotificationConfigSources) {
    let report = doctor_notifications(config.no_notify, config.desktop_mode);
    let paths = resolved_config_paths();
    let color = DoctorColor::detect();

    let status =
        if report.ready { color.green("✓ ready") } else { color.red("✗ action needed") };
    println!("doctor: {status}");

    print_section("settings");
    print_state_row(
        &color,
        report.notifications_enabled,
        "notifications.enabled",
        &format!("{} (source: {})", report.notifications_enabled, sources.enabled.as_str()),
    );
    print_state_row(
        &color,
        true,
        "notifications.desktop",
        &format!("{} (source: {})", report.desktop_mode.as_str(), sources.desktop.as_str()),
    );
    print_neutral_row("platform", report.platform);

    print_section("checks");
    let check_notifications_enabled = report.notifications_enabled;
    print_check_row(
        &color,
        "notifications_enabled",
        check_notifications_enabled,
        if check_notifications_enabled { "pass" } else { "fail" },
    );

    let check_desktop_mode = report.desktop_mode != tapcue::config::DesktopMode::ForceOff;
    print_check_row(
        &color,
        "desktop_mode_allows_notifications",
        check_desktop_mode,
        if check_desktop_mode { "pass" } else { "fail (desktop mode is force-off)" },
    );

    let check_auto_environment = if report.desktop_mode == tapcue::config::DesktopMode::Auto {
        report.auto_environment_ready
    } else {
        true
    };
    print_check_row(
        &color,
        "desktop_environment_ready",
        check_auto_environment,
        if report.desktop_mode == tapcue::config::DesktopMode::Auto {
            if check_auto_environment {
                "pass"
            } else {
                "fail"
            }
        } else {
            "pass (not required in force mode)"
        },
    );

    let check_backend_available = report.backend_command.is_some() && report.backend_found;
    print_check_row(
        &color,
        "backend_available",
        check_backend_available,
        if check_backend_available { "pass" } else { "fail" },
    );
    if let Some(linux_env) = report.linux_environment {
        print_section("env");
        if report.auto_environment_ready {
            print_ok_row(&color, "auto.environment_ready", "true");
        } else {
            print_warn_row(&color, "auto.environment_ready", "false");
        }
        print_neutral_row("env.DISPLAY", if linux_env.display { "set" } else { "unset" });
        print_neutral_row(
            "env.WAYLAND_DISPLAY",
            if linux_env.wayland_display { "set" } else { "unset" },
        );
        print_neutral_row(
            "env.DBUS_SESSION_BUS_ADDRESS",
            if linux_env.dbus_session_bus_address { "set" } else { "unset" },
        );
    }
    print_section("backend");
    if let Some(command) = report.backend_command {
        print_neutral_row("backend.command", command);
    } else {
        print_neutral_row("backend.command", "none");
    }
    if report.backend_found {
        print_ok_row(&color, "backend.found", "true");
    } else {
        print_warn_row(&color, "backend.found", "false");
    }
    print_section("config");

    if let Some(path) = paths.user_config_path {
        print_neutral_row(
            "config.user",
            &format!(
                "{} ({})",
                path.display(),
                if paths.user_config_exists { "found" } else { "missing" }
            ),
        );
    } else {
        print_neutral_row("config.user", "unavailable");
    }
    print_neutral_row(
        "config.local",
        &format!(
            "{} ({})",
            paths.local_config_path.display(),
            if paths.local_config_exists { "found" } else { "missing" }
        ),
    );
    let mut fixes = suggested_fixes(config, &report);
    if !fixes.is_empty() {
        print_section("fixes");
        for (index, fix) in fixes.drain(..).enumerate() {
            print_neutral_row(&format!("fix.{}", index + 1), &fix);
        }
    }

    if !report.reasons.is_empty() {
        print_section("reasons");
        for reason in report.reasons {
            print_fail_row(&color, "reason", &reason);
        }
    }
}

fn print_section(title: &str) {
    println!("{title}:");
}

fn print_check_row(color: &DoctorColor, key: &str, passed: bool, value: &str) {
    if passed {
        print_ok_row(color, key, value);
    } else {
        print_fail_row(color, key, value);
    }
}

fn print_state_row(color: &DoctorColor, good: bool, key: &str, value: &str) {
    if good {
        print_ok_row(color, key, value);
    } else {
        print_warn_row(color, key, value);
    }
}

fn print_ok_row(color: &DoctorColor, key: &str, value: &str) {
    print_row(&color.green("✓"), key, value);
}

fn print_warn_row(color: &DoctorColor, key: &str, value: &str) {
    print_row(&color.yellow("!"), key, value);
}

fn print_fail_row(color: &DoctorColor, key: &str, value: &str) {
    print_row(&color.red("✗"), key, value);
}

fn print_neutral_row(key: &str, value: &str) {
    print_row("-", key, value);
}

fn print_row(icon: &str, key: &str, value: &str) {
    const KEY_WIDTH: usize = 33;
    let dotted_key = format!("{key:.<KEY_WIDTH$}");
    println!("  {icon} {dotted_key} {value}");
}

fn suggested_fixes(
    config: &EffectiveConfig,
    report: &tapcue::notifier::NotificationDoctorReport,
) -> Vec<String> {
    let mut fixes = Vec::new();

    if config.no_notify {
        fixes.push(
            "Enable notifications: remove --no-notify, set TAPCUE_NO_NOTIFY=false, or set notifications.enabled=true"
                .to_owned(),
        );
    }

    if config.desktop_mode == tapcue::config::DesktopMode::ForceOff {
        fixes.push(
            "Use --desktop auto or --desktop force-on (or set notifications.desktop in config)"
                .to_owned(),
        );
    }

    if config.desktop_mode == tapcue::config::DesktopMode::Auto && !report.auto_environment_ready {
        fixes.push(
            "Start from a desktop session with DISPLAY/WAYLAND_DISPLAY/DBUS_SESSION_BUS_ADDRESS available"
                .to_owned(),
        );
    }

    if !report.backend_found {
        match report.backend_command {
            Some("notify-send") => fixes.push(
                "Install notify-send (usually package: libnotify-bin) and ensure it is in PATH"
                    .to_owned(),
            ),
            Some("osascript") => fixes.push(
                "Ensure osascript is available (standard macOS install) or install terminal-notifier, and keep the chosen backend in PATH"
                    .to_owned(),
            ),
            Some("powershell") => fixes.push(
                "Install PowerShell (or restore powershell.exe) and ensure it is in PATH"
                    .to_owned(),
            ),
            Some(other) => fixes.push(format!(
                "Install backend command '{other}' and ensure it is discoverable in PATH"
            )),
            None => fixes.push(
                "Use --no-notify on unsupported platforms or add a platform-specific backend"
                    .to_owned(),
            ),
        }
    }

    fixes
}

#[derive(Clone, Copy)]
struct DoctorColor {
    enabled: bool,
}

impl DoctorColor {
    fn detect() -> Self {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        Self { enabled: !no_color && io::stdout().is_terminal() }
    }

    fn green(self, text: &str) -> String {
        self.paint(text, "32")
    }

    fn red(self, text: &str) -> String {
        self.paint(text, "31")
    }

    fn yellow(self, text: &str) -> String {
        self.paint(text, "33")
    }

    fn paint(self, text: &str, code: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
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
            strict: false,
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
