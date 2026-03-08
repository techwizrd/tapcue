use std::collections::HashSet;
use std::ffi::OsString;

use notify_rust::Notification;

use crate::config::DesktopMode;
use crate::processor::RunState;

pub trait Notifier {
    fn notify_failure(&mut self, label: &str);
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

    fn can_emit_failure(&mut self, label: &str) -> bool {
        const MAX_TRACKED_FAILURE_LABELS: usize = 4096;

        if self.policy.dedup_failures {
            if self.seen_failures.contains(label) {
                return false;
            }

            if self.seen_failures.len() < MAX_TRACKED_FAILURE_LABELS {
                self.seen_failures.insert(label.to_owned());
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
    fn notify_failure(&mut self, label: &str) {
        if self.can_emit_failure(label) {
            self.inner.notify_failure(label);
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
    fn notify_failure(&mut self, _label: &str) {}

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

trait NotificationSender {
    fn send(&self, title: &str, body: &str) -> Result<(), String>;
}

#[derive(Debug, Default)]
struct NotifyRustSender;

impl NotificationSender for NotifyRustSender {
    fn send(&self, title: &str, body: &str) -> Result<(), String> {
        Notification::new()
            .summary(title)
            .body(body)
            .show()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
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
        Self::with_components(
            current_platform(),
            mode,
            Box::new(ProcessEnvironment),
            Box::new(NotifyRustSender),
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

    fn send_notification(&self, title: &str, body: &str) {
        if !self.enabled {
            return;
        }

        if let Err(error) = self.sender.send(title, body) {
            eprintln!("tapcue: failed to send desktop notification: {error}");
        }
    }
}

impl Notifier for DesktopNotifier {
    fn notify_failure(&mut self, label: &str) {
        self.send_notification("TAP failure", label);
    }

    fn notify_bailout(&mut self, reason: &str) {
        self.send_notification("TAP bailout", reason);
    }

    fn notify_summary(&mut self, state: &RunState) {
        let status = if state.is_success() { "success" } else { "failure" };
        let body = format!(
            "status: {status}; total: {}; passed: {}; failed: {}; todo: {}; skipped: {}",
            state.total, state.passed, state.failed, state.todo, state.skipped
        );
        self.send_notification("TAP summary", &body);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};

    use super::{
        desktop_notifications_available, DesktopNotifier, Environment, NotificationPolicy,
        NotificationSender, Platform, PolicyNotifier,
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

    type Notifications = Vec<(String, String)>;

    impl RecordingSender {
        fn shared() -> (Self, Arc<Mutex<Notifications>>) {
            let notifications = Arc::new(Mutex::new(Vec::new()));
            (Self { notifications: Arc::clone(&notifications) }, notifications)
        }
    }

    impl NotificationSender for RecordingSender {
        fn send(&self, title: &str, body: &str) -> Result<(), String> {
            self.notifications
                .lock()
                .expect("lock should not be poisoned")
                .push((title.to_owned(), body.to_owned()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FailingSender;

    impl NotificationSender for FailingSender {
        fn send(&self, _title: &str, _body: &str) -> Result<(), String> {
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

        notifier.notify_failure("boom");
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

        notifier.notify_failure("alpha");
        notifier.notify_bailout("catastrophic");
        notifier.notify_summary(&sample_state());

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 3);
        assert_eq!(notifications[0].0, "TAP failure");
        assert_eq!(notifications[0].1, "alpha");
        assert_eq!(notifications[1].0, "TAP bailout");
        assert_eq!(notifications[1].1, "catastrophic");
        assert_eq!(notifications[2].0, "TAP summary");
        assert!(notifications[2].1.contains("status: failure"));
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
        force_on.notify_failure("forced");
        assert_eq!(shared_on.lock().expect("lock should not be poisoned").len(), 1);

        let (sender_off, shared_off) = RecordingSender::shared();
        let mut force_off = DesktopNotifier::with_components(
            Platform::Linux,
            DesktopMode::ForceOff,
            Box::new(FakeEnvironment::new(&[("DISPLAY", ":0")])),
            Box::new(sender_off),
        );
        force_off.notify_failure("suppressed");
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

        notifier.notify_failure("one");
        notifier.notify_failure("one");
        notifier.notify_failure("two");
        notifier.notify_failure("three");

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].1, "one");
        assert_eq!(notifications[1].1, "two");
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

        notifier.notify_failure("same");
        notifier.notify_failure("same");
        notifier.notify_failure("same");

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

        notifier.notify_failure("alpha");
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

        notifier.notify_failure("one");
        notifier.notify_failure("two");

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
        assert_eq!(notifications[0].0, "TAP bailout");
        assert_eq!(notifications[1].0, "TAP summary");
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
            notifier.notify_failure(&format!("label-{i}"));
        }
        notifier.notify_failure("label-4097");

        let notifications = shared.lock().expect("lock should not be poisoned");
        assert_eq!(notifications.len(), 4201);
    }
}
