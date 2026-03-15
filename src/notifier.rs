use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::config::DesktopMode;
use crate::processor::RunState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureSource {
    Tap,
    Go,
    Nextest,
    Jest,
    Vitest,
}

impl FailureSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tap => "TAP",
            Self::Go => "go",
            Self::Nextest => "nextest",
            Self::Jest => "jest",
            Self::Vitest => "vitest",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailureNotification {
    pub source: FailureSource,
    pub label: String,
    pub suite: Option<String>,
    pub test_file: Option<String>,
    pub reason: Option<String>,
}

impl FailureNotification {
    pub fn new(source: FailureSource, label: impl Into<String>) -> Self {
        Self { source, label: label.into(), suite: None, test_file: None, reason: None }
    }

    pub fn dedup_key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.source.as_str(),
            self.label,
            self.suite.as_deref().unwrap_or(""),
            self.test_file.as_deref().unwrap_or(""),
            self.reason.as_deref().unwrap_or(""),
        )
    }

    pub fn render_body(&self) -> String {
        let mut lines = Vec::new();
        lines.push(format!("Runner: {}", self.source.as_str()));

        if let Some(suite) = self.suite.as_deref().filter(|value| !value.trim().is_empty()) {
            lines.push(format!("Suite: {suite}"));
        }

        if let Some(test_file) = self.test_file.as_deref().filter(|value| !value.trim().is_empty())
        {
            lines.push(format!("File: {test_file}"));
        }

        lines.push(format!("Test: {}", self.label));

        if let Some(reason) = self.reason.as_deref().filter(|value| !value.trim().is_empty()) {
            lines.push(format!("Reason: {reason}"));
        }

        lines.join("\n")
    }
}

pub trait Notifier {
    fn notify_failure(&mut self, failure: &FailureNotification);
    fn notify_bailout(&mut self, reason: &str);
    fn notify_summary(&mut self, state: &RunState);
}

#[derive(Debug, Clone, Copy)]
pub struct NotificationPolicy {
    pub dedup_failures: bool,
    pub max_failure_notifications: Option<usize>,
}

impl Default for NotificationPolicy {
    fn default() -> Self {
        Self { dedup_failures: true, max_failure_notifications: None }
    }
}

pub struct PolicyNotifier<'a> {
    inner: &'a mut dyn Notifier,
    policy: NotificationPolicy,
    seen_failures: HashSet<String>,
    emitted_failure_notifications: usize,
}

impl<'a> PolicyNotifier<'a> {
    pub fn new(inner: &'a mut dyn Notifier, policy: NotificationPolicy) -> Self {
        Self { inner, policy, seen_failures: HashSet::new(), emitted_failure_notifications: 0 }
    }

    fn can_emit_failure(&mut self, failure: &FailureNotification) -> bool {
        const MAX_TRACKED_FAILURE_LABELS: usize = 4096;
        let dedup_key = failure.dedup_key();

        if self.policy.dedup_failures {
            if self.seen_failures.contains(&dedup_key) {
                return false;
            }

            if self.seen_failures.len() < MAX_TRACKED_FAILURE_LABELS {
                self.seen_failures.insert(dedup_key);
            }
        }

        if let Some(limit) = self.policy.max_failure_notifications {
            if self.emitted_failure_notifications >= limit {
                return false;
            }
        }

        self.emitted_failure_notifications += 1;
        true
    }
}

impl Notifier for PolicyNotifier<'_> {
    fn notify_failure(&mut self, failure: &FailureNotification) {
        if self.can_emit_failure(failure) {
            self.inner.notify_failure(failure);
        }
    }

    fn notify_bailout(&mut self, reason: &str) {
        self.inner.notify_bailout(reason);
    }

    fn notify_summary(&mut self, state: &RunState) {
        self.inner.notify_summary(state);
    }
}

#[derive(Debug, Default)]
pub struct NullNotifier;

impl Notifier for NullNotifier {
    fn notify_failure(&mut self, _failure: &FailureNotification) {}

    fn notify_bailout(&mut self, _reason: &str) {}

    fn notify_summary(&mut self, _state: &RunState) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    #[cfg(any(target_os = "linux", test))]
    Linux,
    #[cfg(any(target_os = "macos", test))]
    MacOs,
    #[cfg(any(target_os = "windows", test))]
    Windows,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationKind {
    Failure,
    Bailout,
    SummarySuccess,
    SummaryFailure,
}

fn current_platform() -> Platform {
    #[cfg(target_os = "linux")]
    {
        return Platform::Linux;
    }

    #[cfg(target_os = "macos")]
    {
        return Platform::MacOs;
    }

    #[cfg(target_os = "windows")]
    {
        return Platform::Windows;
    }

    #[allow(unreachable_code)]
    Platform::Other
}

trait Environment {
    fn var_os(&self, key: &str) -> Option<OsString>;
}

#[derive(Debug, Default)]
struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}

fn desktop_notifications_available(platform: Platform, env: &dyn Environment) -> bool {
    match platform {
        #[cfg(any(target_os = "linux", test))]
        Platform::Linux => {
            env.var_os("DISPLAY").is_some()
                || env.var_os("WAYLAND_DISPLAY").is_some()
                || env.var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        }
        #[cfg(any(target_os = "macos", test))]
        Platform::MacOs => true,
        #[cfg(any(target_os = "windows", test))]
        Platform::Windows => true,
        Platform::Other => false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxEnvironmentStatus {
    pub display: bool,
    pub wayland_display: bool,
    pub dbus_session_bus_address: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDoctorReport {
    pub ready: bool,
    pub notifications_enabled: bool,
    pub desktop_mode: DesktopMode,
    pub platform: &'static str,
    pub backend_command: Option<&'static str>,
    pub backend_found: bool,
    pub auto_environment_ready: bool,
    pub linux_environment: Option<LinuxEnvironmentStatus>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct NotificationDoctorSignals {
    platform: Platform,
    linux_environment: LinuxEnvironmentStatus,
    backend_found: bool,
}

pub fn doctor_notifications(no_notify: bool, mode: DesktopMode) -> NotificationDoctorReport {
    let platform = current_platform();
    let signals = NotificationDoctorSignals {
        platform,
        linux_environment: LinuxEnvironmentStatus {
            display: std::env::var_os("DISPLAY").is_some(),
            wayland_display: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            dbus_session_bus_address: std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        },
        backend_found: backend_available_for_platform(platform),
    };

    build_doctor_report(no_notify, mode, signals)
}

fn build_doctor_report(
    no_notify: bool,
    mode: DesktopMode,
    signals: NotificationDoctorSignals,
) -> NotificationDoctorReport {
    let notifications_enabled = !no_notify;
    let auto_environment_ready = match signals.platform {
        #[cfg(any(target_os = "linux", test))]
        Platform::Linux => {
            signals.linux_environment.display
                || signals.linux_environment.wayland_display
                || signals.linux_environment.dbus_session_bus_address
        }
        #[cfg(any(target_os = "macos", test))]
        Platform::MacOs => true,
        #[cfg(any(target_os = "windows", test))]
        Platform::Windows => true,
        Platform::Other => false,
    };

    let mut reasons = Vec::new();
    if !notifications_enabled {
        reasons.push("notifications are disabled by merged configuration".to_owned());
    }

    if mode == DesktopMode::ForceOff {
        reasons.push("desktop mode is force-off".to_owned());
    }

    if notifications_enabled && mode == DesktopMode::Auto && !auto_environment_ready {
        reasons.push("desktop auto-detection did not find an active desktop session".to_owned());
    }

    if notifications_enabled && mode != DesktopMode::ForceOff {
        if let Some(command) = backend_command_for_platform(signals.platform) {
            if !signals.backend_found {
                reasons.push(format!("notification backend command not found in PATH: {command}"));
            }
        } else {
            reasons.push("unsupported platform for desktop notifications".to_owned());
        }
    }

    NotificationDoctorReport {
        ready: reasons.is_empty(),
        notifications_enabled,
        desktop_mode: mode,
        platform: platform_name(signals.platform),
        backend_command: backend_command_for_platform(signals.platform),
        backend_found: signals.backend_found,
        auto_environment_ready,
        linux_environment: match signals.platform {
            #[cfg(any(target_os = "linux", test))]
            Platform::Linux => Some(signals.linux_environment),
            _ => None,
        },
        reasons,
    }
}

fn backend_command_for_platform(platform: Platform) -> Option<&'static str> {
    match platform {
        #[cfg(any(target_os = "linux", test))]
        Platform::Linux => Some("notify-send"),
        #[cfg(any(target_os = "macos", test))]
        Platform::MacOs => Some("osascript"),
        #[cfg(any(target_os = "windows", test))]
        Platform::Windows => Some("powershell"),
        Platform::Other => None,
    }
}

fn backend_available_for_platform(platform: Platform) -> bool {
    match platform {
        #[cfg(any(target_os = "linux", test))]
        Platform::Linux => command_in_path("notify-send"),
        #[cfg(any(target_os = "macos", test))]
        Platform::MacOs => command_in_path("terminal-notifier") || command_in_path("osascript"),
        #[cfg(any(target_os = "windows", test))]
        Platform::Windows => command_in_path("powershell"),
        Platform::Other => false,
    }
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        #[cfg(any(target_os = "linux", test))]
        Platform::Linux => "linux",
        #[cfg(any(target_os = "macos", test))]
        Platform::MacOs => "macos",
        #[cfg(any(target_os = "windows", test))]
        Platform::Windows => "windows",
        Platform::Other => "other",
    }
}

fn command_in_path(command: &str) -> bool {
    let Some(path_value) = std::env::var_os("PATH") else {
        return false;
    };

    for directory in std::env::split_paths(&path_value) {
        if command_exists_at(&directory, command) {
            return true;
        }
    }

    false
}

fn command_exists_at(directory: &Path, command: &str) -> bool {
    let unix_candidate = directory.join(command);
    if unix_candidate.is_file() {
        return true;
    }

    #[cfg(windows)]
    {
        for extension in ["exe", "cmd", "bat"] {
            let candidate = directory.join(format!("{command}.{extension}"));
            if candidate.is_file() {
                return true;
            }
        }
    }

    false
}

trait NotificationSender {
    fn send(&self, kind: NotificationKind, title: &str, body: &str) -> Result<(), String>;
}

#[derive(Debug)]
struct ShellNotificationSender {
    platform: Platform,
}

impl ShellNotificationSender {
    fn new(platform: Platform) -> Self {
        Self { platform }
    }

    fn send_linux(&self, kind: NotificationKind, title: &str, body: &str) -> Result<(), String> {
        let (urgency, icon, expire_ms, stack_key, category) = match kind {
            NotificationKind::Failure => (
                "normal",
                "dialog-warning-symbolic",
                "12000",
                "tapcue-failure",
                "tapcue.test.failure",
            ),
            NotificationKind::Bailout => {
                ("critical", "dialog-error-symbolic", "0", "tapcue-bailout", "tapcue.test.bailout")
            }
            NotificationKind::SummarySuccess => {
                ("normal", "emblem-ok-symbolic", "5000", "tapcue-summary", "tapcue.test.summary")
            }
            NotificationKind::SummaryFailure => (
                "normal",
                "dialog-warning-symbolic",
                "9000",
                "tapcue-summary",
                "tapcue.test.summary",
            ),
        };

        Command::new("notify-send")
            .arg("--app-name")
            .arg("tapcue")
            .arg("--category")
            .arg(category)
            .arg("--urgency")
            .arg(urgency)
            .arg("--icon")
            .arg(icon)
            .arg("--expire-time")
            .arg(expire_ms)
            .arg("--hint")
            .arg(format!("string:x-canonical-private-synchronous:{stack_key}"))
            .arg(title)
            .arg(body)
            .status()
            .map_err(|error| error.to_string())
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("notify-send exited with status {status}"))
                }
            })
    }

    #[cfg(any(target_os = "macos", test))]
    fn send_macos(&self, kind: NotificationKind, title: &str, body: &str) -> Result<(), String> {
        if command_in_path("terminal-notifier") {
            return self.send_macos_terminal_notifier(kind, title, body);
        }

        self.send_macos_osascript(kind, title, body)
    }

    #[cfg(any(target_os = "macos", test))]
    fn send_macos_terminal_notifier(
        &self,
        kind: NotificationKind,
        title: &str,
        body: &str,
    ) -> Result<(), String> {
        let subtitle = macos_subtitle(kind);
        let message = macos_compact_body(kind, body);
        let group = macos_group(kind);

        let mut command = Command::new("terminal-notifier");
        command
            .arg("-title")
            .arg(title)
            .arg("-subtitle")
            .arg(subtitle)
            .arg("-message")
            .arg(message)
            .arg("-group")
            .arg(group);

        if let Some(sound) = macos_sound(kind) {
            command.arg("-sound").arg(sound);
        }

        command.status().map_err(|error| error.to_string()).and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("terminal-notifier exited with status {status}"))
            }
        })
    }

    #[cfg(any(target_os = "macos", test))]
    fn send_macos_osascript(
        &self,
        kind: NotificationKind,
        title: &str,
        body: &str,
    ) -> Result<(), String> {
        let escaped_title = escape_applescript_string(title);
        let escaped_body = escape_applescript_string(&macos_compact_body(kind, body));
        let escaped_subtitle = escape_applescript_string(macos_subtitle(kind));
        let script = format!(
            "display notification \"{escaped_body}\" with title \"{escaped_title}\" subtitle \"{escaped_subtitle}\""
        );

        Command::new("osascript")
            .arg("-e")
            .arg(script)
            .status()
            .map_err(|error| error.to_string())
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("osascript exited with status {status}"))
                }
            })
    }

    #[cfg(any(target_os = "windows", test))]
    fn send_windows(&self, title: &str, body: &str) -> Result<(), String> {
        let escaped_title = title.replace('"', "''");
        let escaped_body = body.replace('"', "''");
        let command = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
            [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] > $null; \
            $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
            $xml.LoadXml('<toast><visual><binding template=\"ToastGeneric\"><text>{escaped_title}</text><text>{escaped_body}</text></binding></visual></toast>'); \
            $toast = [Windows.UI.Notifications.ToastNotification]::new($xml); \
            [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('tapcue').Show($toast);"
        );

        Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(command)
            .status()
            .map_err(|error| error.to_string())
            .and_then(|status| {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("powershell exited with status {status}"))
                }
            })
    }
}

impl NotificationSender for ShellNotificationSender {
    fn send(&self, kind: NotificationKind, title: &str, body: &str) -> Result<(), String> {
        match self.platform {
            #[cfg(any(target_os = "linux", test))]
            Platform::Linux => self.send_linux(kind, title, body),
            #[cfg(any(target_os = "macos", test))]
            Platform::MacOs => self.send_macos(kind, title, body),
            #[cfg(any(target_os = "windows", test))]
            Platform::Windows => self.send_windows(title, body),
            Platform::Other => Err("unsupported platform".to_owned()),
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_subtitle(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Failure => "Test Failure",
        NotificationKind::Bailout => "Run Aborted",
        NotificationKind::SummarySuccess => "Run Summary",
        NotificationKind::SummaryFailure => "Run Summary",
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_group(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Failure => "tapcue-failure",
        NotificationKind::Bailout => "tapcue-bailout",
        NotificationKind::SummarySuccess => "tapcue-summary",
        NotificationKind::SummaryFailure => "tapcue-summary",
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_sound(kind: NotificationKind) -> Option<&'static str> {
    match kind {
        NotificationKind::Bailout => Some("Basso"),
        NotificationKind::SummaryFailure => Some("Funk"),
        NotificationKind::Failure | NotificationKind::SummarySuccess => None,
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_compact_body(kind: NotificationKind, body: &str) -> String {
    match kind {
        NotificationKind::Failure => compact_failure_body_for_macos(body),
        NotificationKind::Bailout
        | NotificationKind::SummarySuccess
        | NotificationKind::SummaryFailure => body.trim().to_owned(),
    }
}

#[cfg(any(target_os = "macos", test))]
fn compact_failure_body_for_macos(body: &str) -> String {
    let mut test_label: Option<&str> = None;
    let mut reason: Option<&str> = None;
    let mut suite: Option<&str> = None;

    for line in body.lines() {
        if test_label.is_none() {
            test_label = line.strip_prefix("Test: ");
        }
        if reason.is_none() {
            reason = line.strip_prefix("Reason: ");
        }
        if suite.is_none() {
            suite = line.strip_prefix("Suite: ");
        }
    }

    if let Some(label) = test_label {
        if let Some(reason) = reason {
            return format!("{label} - {reason}");
        }

        if let Some(suite) = suite {
            return format!("{label} ({suite})");
        }

        return label.to_owned();
    }

    body.lines().next().unwrap_or_default().trim().to_owned()
}

#[cfg(any(target_os = "macos", test))]
fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "")
}

pub struct DesktopNotifier {
    enabled: bool,
    sender: Box<dyn NotificationSender>,
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new(DesktopMode::Auto)
    }
}

impl DesktopNotifier {
    pub fn new(mode: DesktopMode) -> Self {
        let platform = current_platform();
        Self::with_components(
            platform,
            mode,
            Box::new(ProcessEnvironment),
            Box::new(ShellNotificationSender::new(platform)),
        )
    }

    fn with_components(
        platform: Platform,
        mode: DesktopMode,
        environment: Box<dyn Environment>,
        sender: Box<dyn NotificationSender>,
    ) -> Self {
        let enabled = match mode {
            DesktopMode::ForceOn => true,
            DesktopMode::ForceOff => false,
            DesktopMode::Auto => desktop_notifications_available(platform, environment.as_ref()),
        };
        Self { enabled, sender }
    }

    fn send_notification(&self, kind: NotificationKind, title: &str, body: &str) {
        if !self.enabled {
            return;
        }

        if let Err(error) = self.sender.send(kind, title, body) {
            eprintln!("tapcue: failed to send desktop notification: {error}");
        }
    }
}

impl Notifier for DesktopNotifier {
    fn notify_failure(&mut self, failure: &FailureNotification) {
        let title = format!("{} failure", failure.source.as_str());
        let body = failure.render_body();
        self.send_notification(NotificationKind::Failure, &title, &body);
    }

    fn notify_bailout(&mut self, reason: &str) {
        self.send_notification(NotificationKind::Bailout, "TAP bailout", reason);
    }

    fn notify_summary(&mut self, state: &RunState) {
        let status = if state.is_success() { "success" } else { "failure" };
        let body = format!(
            "status: {status}; total: {}; passed: {}; failed: {}; todo: {}; skipped: {}",
            state.total, state.passed, state.failed, state.todo, state.skipped
        );
        let kind = if state.is_success() {
            NotificationKind::SummarySuccess
        } else {
            NotificationKind::SummaryFailure
        };
        self.send_notification(kind, "TAP summary", &body);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};

    use super::{
        build_doctor_report, compact_failure_body_for_macos, desktop_notifications_available,
        escape_applescript_string, macos_compact_body, macos_group, macos_sound, macos_subtitle,
        DesktopNotifier, Environment, FailureNotification, FailureSource, LinuxEnvironmentStatus,
        NotificationDoctorSignals, NotificationKind, NotificationPolicy, NotificationSender,
        Platform, PolicyNotifier,
    };
    use crate::config::DesktopMode;
    use crate::notifier::Notifier;
    use crate::processor::RunState;

    struct FakeEnvironment {
        values: HashMap<String, OsString>,
    }

    impl FakeEnvironment {
        fn new(values: &[(&str, &str)]) -> Self {
            let mut map = HashMap::new();
            for (key, value) in values {
                map.insert((*key).to_owned(), OsString::from(*value));
            }
            Self { values: map }
        }
    }

    impl Environment for FakeEnvironment {
        fn var_os(&self, key: &str) -> Option<OsString> {
            self.values.get(key).cloned()
        }
    }

    #[derive(Default)]
    struct RecordingSender {
        notifications: Arc<Mutex<Notifications>>,
    }

    type Notifications = Vec<(NotificationKind, String, String)>;

    impl RecordingSender {
        fn shared() -> (Self, Arc<Mutex<Notifications>>) {
            let notifications = Arc::new(Mutex::new(Vec::new()));
            (Self { notifications: Arc::clone(&notifications) }, notifications)
        }
    }

    impl NotificationSender for RecordingSender {
        fn send(&self, _kind: NotificationKind, title: &str, body: &str) -> Result<(), String> {
            self.notifications.lock().expect("lock should not be poisoned").push((
                _kind,
                title.to_owned(),
                body.to_owned(),
            ));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingSender;

    impl NotificationSender for FailingSender {
        fn send(&self, _kind: NotificationKind, _title: &str, _body: &str) -> Result<(), String> {
            Err("boom".to_owned())
        }
    }

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

    #[test]
    fn linux_requires_desktop_session_markers() {
        let empty = FakeEnvironment::new(&[]);
        assert!(!desktop_notifications_available(Platform::Linux, &empty));

        let with_display = FakeEnvironment::new(&[("DISPLAY", ":0")]);
        assert!(desktop_notifications_available(Platform::Linux, &with_display));

        let with_wayland = FakeEnvironment::new(&[("WAYLAND_DISPLAY", "wayland-0")]);
        assert!(desktop_notifications_available(Platform::Linux, &with_wayland));

        let with_dbus = FakeEnvironment::new(&[("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/bus")]);
        assert!(desktop_notifications_available(Platform::Linux, &with_dbus));
    }

    #[test]
    fn macos_and_windows_are_enabled_without_extra_env() {
        let empty = FakeEnvironment::new(&[]);
        assert!(desktop_notifications_available(Platform::MacOs, &empty));
        assert!(desktop_notifications_available(Platform::Windows, &empty));
        assert!(!desktop_notifications_available(Platform::Other, &empty));
    }

    #[test]
    fn disabled_environment_suppresses_notifications() {
        let (sender, shared) = RecordingSender::shared();
        let mut notifier = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::Auto,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "boom"));
        notifier.notify_bailout("stopped");
        notifier.notify_summary(&sample_state());

        assert!(shared.lock().expect("lock should not be poisoned").is_empty());
    }

    #[test]
    fn enabled_environment_emits_failure_bailout_and_summary() {
        let (sender, shared) = RecordingSender::shared();
        let mut notifier = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::Auto,
            Box::new(FakeEnvironment::new(&[("DISPLAY", ":0")])),
            Box::new(sender),
        );

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "alpha"));
        notifier.notify_bailout("catastrophic");
        notifier.notify_summary(&sample_state());

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 3);
        assert_eq!(notifications[0].0, NotificationKind::Failure);
        assert_eq!(notifications[0].1, "TAP failure");
        assert!(notifications[0].2.contains("Runner: TAP"));
        assert!(notifications[0].2.contains("Test: alpha"));
        assert_eq!(notifications[1].0, NotificationKind::Bailout);
        assert_eq!(notifications[1].1, "TAP bailout");
        assert_eq!(notifications[1].2, "catastrophic");
        assert_eq!(notifications[2].0, NotificationKind::SummaryFailure);
        assert_eq!(notifications[2].1, "TAP summary");
        assert!(notifications[2].2.contains("status: failure"));
    }

    #[test]
    fn force_modes_override_environment_detection() {
        let (sender_on, shared_on) = RecordingSender::shared();
        let mut force_on = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender_on),
        );
        force_on.notify_failure(&FailureNotification::new(FailureSource::Tap, "forced"));
        assert_eq!(shared_on.lock().expect("lock should not be poisoned").len(), 1);

        let (sender_off, shared_off) = RecordingSender::shared();
        let mut force_off = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOff,
            Box::new(FakeEnvironment::new(&[("DISPLAY", ":0")])),
            Box::new(sender_off),
        );
        force_off.notify_failure(&FailureNotification::new(FailureSource::Tap, "suppressed"));
        assert!(shared_off.lock().expect("lock should not be poisoned").is_empty());
    }

    #[test]
    fn policy_notifier_deduplicates_failures_and_honors_limit() {
        let (sender, shared) = RecordingSender::shared();
        let mut desktop = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );
        let policy =
            NotificationPolicy { dedup_failures: true, max_failure_notifications: Some(2) };
        let mut notifier = PolicyNotifier::new(&mut desktop, policy);

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "one"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "one"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "two"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "three"));

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 2);
        assert!(notifications[0].2.contains("Test: one"));
        assert!(notifications[1].2.contains("Test: two"));
    }

    #[test]
    fn failure_body_uses_clean_multiline_layout() {
        let mut failure = FailureNotification::new(FailureSource::Go, "TestHTTP");
        failure.suite = Some("pkg/server".to_owned());
        failure.test_file = Some("server/http_test.go".to_owned());
        failure.reason = Some("expected 200, got 500".to_owned());

        let rendered = failure.render_body();
        assert!(rendered.contains("Runner: go"));
        assert!(rendered.contains("Suite: pkg/server"));
        assert!(rendered.contains("File: server/http_test.go"));
        assert!(rendered.contains("Test: TestHTTP"));
        assert!(rendered.contains("Reason: expected 200, got 500"));
    }

    #[test]
    fn macos_failure_body_prefers_test_and_reason() {
        let full_body =
            "Runner: go\nSuite: pkg/server\nFile: server/http_test.go\nTest: TestHTTP\nReason: expected 200, got 500";
        let compact = compact_failure_body_for_macos(full_body);
        assert_eq!(compact, "TestHTTP - expected 200, got 500");
    }

    #[test]
    fn macos_failure_body_falls_back_to_suite_when_reason_missing() {
        let full_body = "Runner: go\nSuite: pkg/server\nTest: TestHTTP";
        let compact = compact_failure_body_for_macos(full_body);
        assert_eq!(compact, "TestHTTP (pkg/server)");
    }

    #[test]
    fn macos_compact_body_keeps_summary_readable() {
        let summary = "status: failure; total: 3; passed: 2; failed: 1";
        assert_eq!(macos_compact_body(NotificationKind::SummaryFailure, summary), summary);
    }

    #[test]
    fn macos_metadata_is_kind_specific() {
        assert_eq!(macos_subtitle(NotificationKind::Failure), "Test Failure");
        assert_eq!(macos_subtitle(NotificationKind::Bailout), "Run Aborted");
        assert_eq!(macos_group(NotificationKind::Failure), "tapcue-failure");
        assert_eq!(macos_group(NotificationKind::SummarySuccess), "tapcue-summary");
        assert_eq!(macos_sound(NotificationKind::Failure), None);
        assert_eq!(macos_sound(NotificationKind::Bailout), Some("Basso"));
    }

    #[test]
    fn applescript_escaping_handles_quotes_backslashes_and_newlines() {
        let raw = "a\"b\\c\nline2\r";
        let escaped = escape_applescript_string(raw);
        assert_eq!(escaped, "a\\\"b\\\\c\\nline2");
    }

    #[test]
    fn policy_notifier_without_dedup_allows_repeated_labels() {
        let (sender, shared) = RecordingSender::shared();
        let mut desktop = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );
        let mut notifier = PolicyNotifier::new(
            &mut desktop,
            NotificationPolicy { dedup_failures: false, max_failure_notifications: Some(3) },
        );

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "same"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "same"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "same"));

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 3);
    }

    #[test]
    fn desktop_notifier_handles_sender_errors_without_panicking() {
        let mut notifier = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(FailingSender),
        );

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "alpha"));
        notifier.notify_bailout("stop");
        notifier.notify_summary(&sample_state());
    }

    #[test]
    fn policy_notifier_zero_limit_suppresses_all_failures() {
        let (sender, shared) = RecordingSender::shared();
        let mut desktop = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );
        let mut notifier = PolicyNotifier::new(
            &mut desktop,
            NotificationPolicy { dedup_failures: false, max_failure_notifications: Some(0) },
        );

        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "one"));
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "two"));

        assert!(shared.lock().expect("lock should not be poisoned").is_empty());
    }

    #[test]
    fn policy_notifier_forwards_bailout_and_summary() {
        let (sender, shared) = RecordingSender::shared();
        let mut desktop = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );
        let mut notifier = PolicyNotifier::new(&mut desktop, NotificationPolicy::default());

        notifier.notify_bailout("catastrophic");
        notifier.notify_summary(&sample_state());

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].0, NotificationKind::Bailout);
        assert_eq!(notifications[0].1, "TAP bailout");
        assert_eq!(notifications[1].0, NotificationKind::SummaryFailure);
        assert_eq!(notifications[1].1, "TAP summary");
    }

    #[test]
    fn successful_summary_uses_success_kind() {
        let (sender, shared) = RecordingSender::shared();
        let mut notifier = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );

        let state = RunState {
            planned: Some(1),
            total: 1,
            passed: 1,
            failed: 0,
            todo: 0,
            skipped: 0,
            bailout_reason: None,
            parse_warning_count: 0,
            protocol_failures: 0,
        };

        notifier.notify_summary(&state);

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].0, NotificationKind::SummarySuccess);
        assert_eq!(notifications[0].1, "TAP summary");
    }

    #[test]
    fn dedup_tracking_cap_falls_back_to_limit_only() {
        let (sender, shared) = RecordingSender::shared();
        let mut desktop = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOn,
            Box::new(FakeEnvironment::new(&[])),
            Box::new(sender),
        );
        let mut notifier = PolicyNotifier::new(
            &mut desktop,
            NotificationPolicy { dedup_failures: true, max_failure_notifications: Some(5000) },
        );

        for i in 0..4200 {
            notifier.notify_failure(&FailureNotification::new(
                FailureSource::Tap,
                format!("label-{i}"),
            ));
        }
        notifier.notify_failure(&FailureNotification::new(FailureSource::Tap, "label-4097"));

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 4201);
    }

    #[test]
    fn doctor_reports_not_ready_when_notifications_are_disabled() {
        let report = build_doctor_report(
            true,
            DesktopMode::Auto,
            NotificationDoctorSignals {
                platform: Platform::Linux,
                linux_environment: LinuxEnvironmentStatus {
                    display: true,
                    wayland_display: false,
                    dbus_session_bus_address: false,
                },
                backend_found: true,
            },
        );

        assert!(!report.ready);
        assert!(report.reasons.iter().any(|reason| reason.contains("disabled")));
    }

    #[test]
    fn doctor_reports_not_ready_when_linux_auto_detection_fails() {
        let report = build_doctor_report(
            false,
            DesktopMode::Auto,
            NotificationDoctorSignals {
                platform: Platform::Linux,
                linux_environment: LinuxEnvironmentStatus {
                    display: false,
                    wayland_display: false,
                    dbus_session_bus_address: false,
                },
                backend_found: true,
            },
        );

        assert!(!report.ready);
        assert!(report.reasons.iter().any(|reason| reason.contains("auto-detection")));
    }

    #[test]
    fn doctor_reports_not_ready_when_backend_command_is_missing() {
        let report = build_doctor_report(
            false,
            DesktopMode::ForceOn,
            NotificationDoctorSignals {
                platform: Platform::Linux,
                linux_environment: LinuxEnvironmentStatus {
                    display: true,
                    wayland_display: false,
                    dbus_session_bus_address: false,
                },
                backend_found: false,
            },
        );

        assert!(!report.ready);
        assert!(report.reasons.iter().any(|reason| reason.contains("PATH")));
    }
}
