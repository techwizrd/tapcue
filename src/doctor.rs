use std::io;
use std::io::IsTerminal;

use tapcue::config::{ConfigPathInfo, DesktopMode, EffectiveConfig, NotificationConfigSources};
use tapcue::notifier::{NotificationDoctorReport, doctor_notifications};

pub(crate) fn emit_doctor(config: &EffectiveConfig, sources: &NotificationConfigSources) {
    let report = doctor_notifications(config.no_notify, config.desktop_mode);
    let paths = tapcue::config::resolved_config_paths();
    let color = DoctorColor::detect();

    let status =
        if report.ready { color.green("✓ ready") } else { color.red("✗ action needed") };
    println!("doctor: {status}");

    emit_doctor_settings(&color, &report, sources);
    emit_doctor_checks(&color, &report);
    emit_doctor_env(&color, &report);
    emit_doctor_backend(&color, &report);
    emit_doctor_config(config, &report, &paths);
    emit_doctor_reasons(&color, &report);
}

pub(crate) fn emit_doctor_notifications(config: &EffectiveConfig) {
    let report = doctor_notifications(config.no_notify, config.desktop_mode);
    let color = DoctorColor::detect();

    let status = if report.ready {
        color.green("✓ ready to send notifications")
    } else {
        color.red("✗ setup needed")
    };

    println!("doctor --notifications: {status}");

    print_section("next steps");
    let mut steps = suggested_fixes(config, &report);
    if steps.is_empty() {
        steps.push("Run `tapcue run -- <your-test-command>` once setup is complete".to_owned());
    }
    for (index, step) in steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step);
    }

    println!("\nquick test:");
    println!("  tapcue run -- pytest");

    if !report.ready {
        println!("\nre-run after each fix:");
        println!("  tapcue doctor --notifications");
    }
}

fn emit_doctor_settings(
    color: &DoctorColor,
    report: &NotificationDoctorReport,
    sources: &NotificationConfigSources,
) {
    print_section("settings");
    print_state_row(
        color,
        report.notifications_enabled,
        "notifications.enabled",
        &format!("{} (source: {})", report.notifications_enabled, sources.enabled.as_str()),
    );
    print_state_row(
        color,
        true,
        "notifications.desktop",
        &format!("{} (source: {})", report.desktop_mode.as_str(), sources.desktop.as_str()),
    );
    print_neutral_row("platform", report.platform);
}

fn emit_doctor_checks(color: &DoctorColor, report: &NotificationDoctorReport) {
    print_section("checks");
    let check_notifications_enabled = report.notifications_enabled;
    print_check_row(
        color,
        "notifications_enabled",
        check_notifications_enabled,
        if check_notifications_enabled { "pass" } else { "fail" },
    );

    let check_desktop_mode = report.desktop_mode != DesktopMode::ForceOff;
    print_check_row(
        color,
        "desktop_mode_allows_notifications",
        check_desktop_mode,
        if check_desktop_mode { "pass" } else { "fail (desktop mode is force-off)" },
    );

    let check_auto_environment =
        if report.desktop_mode == DesktopMode::Auto { report.auto_environment_ready } else { true };
    let auto_environment_detail = if report.desktop_mode == DesktopMode::Auto {
        if check_auto_environment { "pass" } else { "fail" }
    } else {
        "pass (not required in force mode)"
    };
    print_check_row(
        color,
        "desktop_environment_ready",
        check_auto_environment,
        auto_environment_detail,
    );

    let check_backend_available = report.backend_command.is_some() && report.backend_found;
    print_check_row(
        color,
        "backend_available",
        check_backend_available,
        if check_backend_available { "pass" } else { "fail" },
    );
}

fn emit_doctor_env(color: &DoctorColor, report: &NotificationDoctorReport) {
    if let Some(linux_env) = report.linux_environment {
        print_section("env");
        if report.auto_environment_ready {
            print_ok_row(color, "auto.environment_ready", "true");
        } else {
            print_warn_row(color, "auto.environment_ready", "false");
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
}

fn emit_doctor_backend(color: &DoctorColor, report: &NotificationDoctorReport) {
    print_section("backend");
    if let Some(command) = report.backend_command {
        print_neutral_row("backend.command", command);
    } else {
        print_neutral_row("backend.command", "none");
    }
    if report.backend_found {
        print_ok_row(color, "backend.found", "true");
    } else {
        print_warn_row(color, "backend.found", "false");
    }
}

fn emit_doctor_config(
    config: &EffectiveConfig,
    report: &NotificationDoctorReport,
    paths: &ConfigPathInfo,
) {
    print_section("config");

    if let Some(path) = &paths.user_config_path {
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

    let mut fixes = suggested_fixes(config, report);
    if !fixes.is_empty() {
        print_section("fixes");
        for (index, fix) in fixes.drain(..).enumerate() {
            print_neutral_row(&format!("fix.{}", index + 1), &fix);
        }
    }
}

fn emit_doctor_reasons(color: &DoctorColor, report: &NotificationDoctorReport) {
    if !report.reasons.is_empty() {
        print_section("reasons");
        for reason in &report.reasons {
            print_fail_row(color, "reason", reason);
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

fn suggested_fixes(config: &EffectiveConfig, report: &NotificationDoctorReport) -> Vec<String> {
    let mut fixes = Vec::new();

    if config.no_notify {
        fixes.push(
            "Enable notifications: remove --no-notify, set TAPCUE_NO_NOTIFY=false, or set notifications.enabled=true"
                .to_owned(),
        );
    }

    if config.desktop_mode == DesktopMode::ForceOff {
        fixes.push(
            "Use --desktop auto or --desktop force-on (or set notifications.desktop in config)"
                .to_owned(),
        );
    }

    if config.desktop_mode == DesktopMode::Auto && !report.auto_environment_ready {
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
                "Install PowerShell (or restore powershell.exe) and ensure it is in PATH".to_owned(),
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
        if self.enabled { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_owned() }
    }
}
