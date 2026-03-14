//! Configuration loading for `tapcue`.
//!
//! Sources are merged in this order (lowest to highest precedence):
//! 1. built-in defaults
//! 2. user config file (`config.toml` in the platform config directory)
//! 3. local project config file (`./.tapcue.toml`)
//! 4. environment variables
//! 5. CLI flags

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::cli::Cli;

const LOCAL_CONFIG_NAME: &str = ".tapcue.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    UserConfig,
    LocalConfig,
    Environment,
    Cli,
}

impl ConfigSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::UserConfig => "user-config",
            Self::LocalConfig => "local-config",
            Self::Environment => "environment",
            Self::Cli => "cli",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationConfigSources {
    pub enabled: ConfigSource,
    pub desktop: ConfigSource,
}

impl Default for NotificationConfigSources {
    fn default() -> Self {
        Self { enabled: ConfigSource::Default, desktop: ConfigSource::Default }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigPathInfo {
    pub user_config_path: Option<PathBuf>,
    pub user_config_exists: bool,
    pub local_config_path: PathBuf,
    pub local_config_exists: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DesktopMode {
    #[default]
    Auto,
    ForceOn,
    ForceOff,
}

impl DesktopMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ForceOn => "force-on",
            Self::ForceOff => "force-off",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputFormat {
    #[default]
    Auto,
    Tap,
    Json,
}

impl InputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Tap => "tap",
            Self::Json => "json",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SummaryFormat {
    #[default]
    None,
    Text,
    Json,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EffectiveConfig {
    pub quiet_parse_errors: bool,
    pub no_notify: bool,
    pub desktop_mode: DesktopMode,
    pub input_format: InputFormat,
    pub summary_format: SummaryFormat,
    pub summary_file: Option<PathBuf>,
    pub dedup_failures: bool,
    pub max_failure_notifications: Option<usize>,
    pub trace_detection: bool,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            quiet_parse_errors: false,
            no_notify: false,
            desktop_mode: DesktopMode::Auto,
            input_format: InputFormat::Auto,
            summary_format: SummaryFormat::None,
            summary_file: None,
            dedup_failures: true,
            max_failure_notifications: None,
            trace_detection: false,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    parser: ParserConfig,
    #[serde(default)]
    input: InputConfig,
    #[serde(default)]
    notifications: NotificationsConfig,
    #[serde(default)]
    output: OutputConfig,
}

#[derive(Debug, Default, Deserialize)]
struct ParserConfig {
    quiet_parse_errors: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct InputConfig {
    format: Option<InputFormat>,
}

#[derive(Debug, Default, Deserialize)]
struct NotificationsConfig {
    enabled: Option<bool>,
    desktop: Option<DesktopMode>,
    dedup_failures: Option<bool>,
    max_failure_notifications: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct OutputConfig {
    summary_format: Option<SummaryFormat>,
    summary_file: Option<PathBuf>,
}

impl EffectiveConfig {
    pub fn load(cli: &Cli) -> Result<Self> {
        let (effective, _) = Self::load_with_sources(cli)?;
        Ok(effective)
    }

    pub fn load_with_sources(cli: &Cli) -> Result<(Self, NotificationConfigSources)> {
        let mut merged = EffectiveConfig::default();
        let mut notification_sources = NotificationConfigSources::default();

        if let Some(path) = user_config_path() {
            merged.merge_file_internal(
                &path,
                Some(ConfigSource::UserConfig),
                Some(&mut notification_sources),
            )?;
        }

        merged.merge_file_internal(
            Path::new(LOCAL_CONFIG_NAME),
            Some(ConfigSource::LocalConfig),
            Some(&mut notification_sources),
        )?;
        merged.merge_env_internal(Some(&mut notification_sources));
        merged.merge_cli_internal(cli, Some(&mut notification_sources));

        Ok((merged, notification_sources))
    }

    #[cfg(test)]
    fn merge_file(&mut self, path: &Path) -> Result<()> {
        self.merge_file_internal(path, None, None)
    }

    fn merge_file_internal(
        &mut self,
        path: &Path,
        source: Option<ConfigSource>,
        notification_sources: Option<&mut NotificationConfigSources>,
    ) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let mut notification_sources = notification_sources;

        let raw_source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let file_config: FileConfig = toml::from_str(&raw_source)
            .with_context(|| format!("failed to parse TOML config file {}", path.display()))?;

        if let Some(value) = file_config.parser.quiet_parse_errors {
            self.quiet_parse_errors = value;
        }

        if let Some(value) = file_config.notifications.enabled {
            self.no_notify = !value;
            if let (Some(source), Some(sources)) = (source, notification_sources.as_deref_mut()) {
                sources.enabled = source;
            }
        }

        if let Some(value) = file_config.notifications.desktop {
            self.desktop_mode = value;
            if let (Some(source), Some(sources)) = (source, notification_sources.as_deref_mut()) {
                sources.desktop = source;
            }
        }

        if let Some(value) = file_config.notifications.dedup_failures {
            self.dedup_failures = value;
        }

        if let Some(value) = file_config.notifications.max_failure_notifications {
            self.max_failure_notifications = Some(value);
        }

        if let Some(value) = file_config.input.format {
            self.input_format = value;
        }

        if let Some(value) = file_config.output.summary_format {
            self.summary_format = value;
        }

        if let Some(value) = file_config.output.summary_file {
            self.summary_file = Some(value);
        }

        Ok(())
    }

    #[cfg(test)]
    fn merge_env(&mut self) {
        self.merge_env_internal(None);
    }

    fn merge_env_internal(&mut self, notification_sources: Option<&mut NotificationConfigSources>) {
        let mut notification_sources = notification_sources;

        if let Some(value) = read_env_bool("TAPCUE_QUIET_PARSE_ERRORS") {
            self.quiet_parse_errors = value;
        }

        if let Some(value) = read_env_bool("TAPCUE_NO_NOTIFY") {
            self.no_notify = value;
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.enabled = ConfigSource::Environment;
            }
        }

        if let Some(value) = read_env_bool("TAPCUE_NOTIFICATIONS_ENABLED") {
            self.no_notify = !value;
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.enabled = ConfigSource::Environment;
            }
        }

        if let Some(value) = read_env_desktop_mode("TAPCUE_DESKTOP") {
            self.desktop_mode = value;
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.desktop = ConfigSource::Environment;
            }
        }

        if let Some(value) = read_env_input_format("TAPCUE_FORMAT") {
            self.input_format = value;
        }

        if let Some(value) = read_env_summary_format("TAPCUE_SUMMARY_FORMAT") {
            self.summary_format = value;
        }

        if let Ok(value) = env::var("TAPCUE_SUMMARY_FILE") {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                self.summary_file = Some(PathBuf::from(trimmed));
            }
        }

        if let Some(value) = read_env_bool("TAPCUE_DEDUP_FAILURES") {
            self.dedup_failures = value;
        }

        if let Some(value) = read_env_usize("TAPCUE_MAX_FAILURE_NOTIFICATIONS") {
            self.max_failure_notifications = Some(value);
        }

        if let Some(value) = read_env_bool("TAPCUE_TRACE_DETECTION") {
            self.trace_detection = value;
        }
    }

    #[cfg(test)]
    fn merge_cli(&mut self, cli: &Cli) {
        self.merge_cli_internal(cli, None);
    }

    fn merge_cli_internal(
        &mut self,
        cli: &Cli,
        notification_sources: Option<&mut NotificationConfigSources>,
    ) {
        let mut notification_sources = notification_sources;

        if cli.quiet_parse_errors {
            self.quiet_parse_errors = true;
        }

        if cli.no_quiet_parse_errors {
            self.quiet_parse_errors = false;
        }

        if cli.no_notify {
            self.no_notify = true;
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.enabled = ConfigSource::Cli;
            }
        }

        if cli.notify {
            self.no_notify = false;
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.enabled = ConfigSource::Cli;
            }
        }

        if let Some(value) = cli.desktop {
            self.desktop_mode = value.into();
            if let Some(sources) = notification_sources.as_deref_mut() {
                sources.desktop = ConfigSource::Cli;
            }
        }

        if let Some(value) = cli.format {
            self.input_format = value.into();
        }

        if let Some(value) = cli.summary_format {
            self.summary_format = value.into();
        }

        if let Some(value) = &cli.summary_file {
            self.summary_file = Some(PathBuf::from(value));
        }

        if cli.dedup_failures {
            self.dedup_failures = true;
        }

        if cli.no_dedup_failures {
            self.dedup_failures = false;
        }

        if let Some(value) = cli.max_failure_notifications {
            self.max_failure_notifications = Some(value);
        }

        if cli.trace_detection {
            self.trace_detection = true;
        }
    }

    pub fn to_pretty_toml(&self) -> Result<String> {
        let rendered = RenderedConfig {
            parser: RenderedParserConfig { quiet_parse_errors: self.quiet_parse_errors },
            input: RenderedInputConfig { format: self.input_format },
            notifications: RenderedNotificationsConfig {
                enabled: !self.no_notify,
                desktop: self.desktop_mode,
                dedup_failures: self.dedup_failures,
                max_failure_notifications: self.max_failure_notifications,
            },
            output: RenderedOutputConfig {
                summary_format: self.summary_format,
                summary_file: self.summary_file.clone(),
            },
        };

        toml::to_string_pretty(&rendered).context("failed to render effective config as TOML")
    }
}

pub fn resolved_config_paths() -> ConfigPathInfo {
    let user_path = user_config_path();
    let user_exists = user_path.as_ref().is_some_and(|path| path.exists());
    let local_path = PathBuf::from(LOCAL_CONFIG_NAME);
    let local_exists = local_path.exists();

    ConfigPathInfo {
        user_config_path: user_path,
        user_config_exists: user_exists,
        local_config_path: local_path,
        local_config_exists: local_exists,
    }
}

#[derive(Serialize)]
struct RenderedConfig {
    parser: RenderedParserConfig,
    input: RenderedInputConfig,
    notifications: RenderedNotificationsConfig,
    output: RenderedOutputConfig,
}

#[derive(Serialize)]
struct RenderedParserConfig {
    quiet_parse_errors: bool,
}

#[derive(Serialize)]
struct RenderedInputConfig {
    format: InputFormat,
}

#[derive(Serialize)]
struct RenderedNotificationsConfig {
    enabled: bool,
    desktop: DesktopMode,
    dedup_failures: bool,
    max_failure_notifications: Option<usize>,
}

#[derive(Serialize)]
struct RenderedOutputConfig {
    summary_format: SummaryFormat,
    summary_file: Option<PathBuf>,
}

fn user_config_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "tapcue")?;
    Some(dirs.config_dir().join("config.toml"))
}

fn read_env_bool(key: &str) -> Option<bool> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            eprintln!("tapcue: invalid boolean in {key}: {value}");
            None
        }
    }
}

fn read_env_desktop_mode(key: &str) -> Option<DesktopMode> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(DesktopMode::Auto),
        "force-on" | "force_on" | "on" | "enabled" => Some(DesktopMode::ForceOn),
        "force-off" | "force_off" | "off" | "disabled" => Some(DesktopMode::ForceOff),
        _ => {
            eprintln!("tapcue: invalid desktop mode in {key}: {value}");
            None
        }
    }
}

fn read_env_input_format(key: &str) -> Option<InputFormat> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Some(InputFormat::Auto),
        "tap" => Some(InputFormat::Tap),
        "json" => Some(InputFormat::Json),
        _ => {
            eprintln!("tapcue: invalid format in {key}: {value}");
            None
        }
    }
}

fn read_env_summary_format(key: &str) -> Option<SummaryFormat> {
    let value = env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => Some(SummaryFormat::None),
        "text" => Some(SummaryFormat::Text),
        "json" => Some(SummaryFormat::Json),
        _ => {
            eprintln!("tapcue: invalid summary format in {key}: {value}");
            None
        }
    }
}

fn read_env_usize(key: &str) -> Option<usize> {
    let value = env::var(key).ok()?;
    match value.trim().parse::<usize>() {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            eprintln!("tapcue: invalid integer in {key}: {value}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use tempfile::tempdir;

    use super::{
        ConfigSource, DesktopMode, EffectiveConfig, InputFormat, NotificationConfigSources,
        SummaryFormat,
    };
    use crate::cli::Cli;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct ScopedEnv {
        key: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnv {
        fn set(key: &'static str, value: &'static str) -> Self {
            let previous = env::var(key).ok();
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnv {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn config_file_merges_fields() {
        let dir = tempdir().expect("temp dir should create");
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "[parser]\nquiet_parse_errors = true\n[notifications]\nenabled = false\ndesktop = \"force-on\"\n",
        )
        .expect("config file should write");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_file(&path).expect("config should parse");

        assert!(cfg.quiet_parse_errors);
        assert!(cfg.no_notify);
        assert_eq!(cfg.desktop_mode, DesktopMode::ForceOn);
    }

    #[test]
    fn cli_overrides_supported_values() {
        let mut cfg = EffectiveConfig {
            quiet_parse_errors: false,
            no_notify: true,
            desktop_mode: DesktopMode::ForceOff,
            input_format: InputFormat::Tap,
            summary_format: SummaryFormat::None,
            summary_file: None,
            dedup_failures: true,
            max_failure_notifications: None,
            trace_detection: false,
        };

        let cli = Cli {
            quiet_parse_errors: true,
            no_quiet_parse_errors: false,
            no_notify: false,
            notify: true,
            desktop: Some(crate::cli::CliDesktopMode::Auto),
            format: Some(crate::cli::CliInputFormat::Json),
            summary_format: Some(crate::cli::CliSummaryFormat::Json),
            summary_file: Some("summary.json".to_owned()),
            dedup_failures: false,
            no_dedup_failures: true,
            max_failure_notifications: Some(4),
            trace_detection: true,
            validate_config: false,
            print_effective_config: false,
            doctor: false,
        };
        cfg.merge_cli(&cli);

        assert!(cfg.quiet_parse_errors);
        assert!(!cfg.no_notify);
        assert_eq!(cfg.desktop_mode, DesktopMode::Auto);
        assert_eq!(cfg.input_format, InputFormat::Json);
        assert_eq!(cfg.summary_format, SummaryFormat::Json);
        assert_eq!(cfg.summary_file.as_deref(), Some(Path::new("summary.json")));
        assert!(!cfg.dedup_failures);
        assert_eq!(cfg.max_failure_notifications, Some(4));
        assert!(cfg.trace_detection);
    }

    #[test]
    fn precedence_is_cli_then_env_then_local_then_user_then_defaults() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let dir = tempdir().expect("temp dir should create");
        let user_path = dir.path().join("user.toml");
        let local_path = dir.path().join("local.toml");

        fs::write(
            &user_path,
            "[parser]\nquiet_parse_errors = false\n[notifications]\nenabled = true\ndesktop = \"auto\"\n",
        )
        .expect("user config should write");
        fs::write(
            &local_path,
            "[parser]\nquiet_parse_errors = true\n[notifications]\nenabled = false\ndesktop = \"force-on\"\n",
        )
        .expect("local config should write");

        let _env_quiet = ScopedEnv::set("TAPCUE_QUIET_PARSE_ERRORS", "false");
        let _env_notify = ScopedEnv::set("TAPCUE_NO_NOTIFY", "false");
        let _env_desktop = ScopedEnv::set("TAPCUE_DESKTOP", "force-off");

        let cli = Cli {
            quiet_parse_errors: true,
            no_quiet_parse_errors: false,
            no_notify: true,
            notify: false,
            desktop: Some(crate::cli::CliDesktopMode::Auto),
            format: Some(crate::cli::CliInputFormat::Auto),
            summary_format: None,
            summary_file: None,
            dedup_failures: false,
            no_dedup_failures: false,
            max_failure_notifications: None,
            trace_detection: false,
            validate_config: false,
            print_effective_config: false,
            doctor: false,
        };

        let mut cfg = EffectiveConfig::default();
        cfg.merge_file(&user_path).expect("user config should parse");
        cfg.merge_file(&local_path).expect("local config should parse");
        cfg.merge_env();
        cfg.merge_cli(&cli);

        assert!(cfg.quiet_parse_errors);
        assert!(cfg.no_notify);
        assert_eq!(cfg.desktop_mode, DesktopMode::Auto);
        assert_eq!(cfg.input_format, InputFormat::Auto);
    }

    #[test]
    fn cli_can_force_false_over_env_true() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _env_quiet = ScopedEnv::set("TAPCUE_QUIET_PARSE_ERRORS", "true");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();
        assert!(cfg.quiet_parse_errors);

        let cli = Cli {
            quiet_parse_errors: false,
            no_quiet_parse_errors: true,
            no_notify: false,
            notify: false,
            desktop: None,
            format: None,
            summary_format: None,
            summary_file: None,
            dedup_failures: false,
            no_dedup_failures: false,
            max_failure_notifications: None,
            trace_detection: false,
            validate_config: false,
            print_effective_config: false,
            doctor: false,
        };
        cfg.merge_cli(&cli);
        assert!(!cfg.quiet_parse_errors);
    }

    #[test]
    fn env_can_set_input_format() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _env_format = ScopedEnv::set("TAPCUE_FORMAT", "json");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();
        assert_eq!(cfg.input_format, InputFormat::Json);
    }

    #[test]
    fn env_can_set_output_and_notification_options() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _summary_format = ScopedEnv::set("TAPCUE_SUMMARY_FORMAT", "text");
        let _summary_file = ScopedEnv::set("TAPCUE_SUMMARY_FILE", "report.txt");
        let _dedup = ScopedEnv::set("TAPCUE_DEDUP_FAILURES", "false");
        let _max_fail = ScopedEnv::set("TAPCUE_MAX_FAILURE_NOTIFICATIONS", "3");
        let _trace = ScopedEnv::set("TAPCUE_TRACE_DETECTION", "true");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();

        assert_eq!(cfg.summary_format, SummaryFormat::Text);
        assert_eq!(cfg.summary_file.as_deref(), Some(Path::new("report.txt")));
        assert!(!cfg.dedup_failures);
        assert_eq!(cfg.max_failure_notifications, Some(3));
        assert!(cfg.trace_detection);
    }

    #[test]
    fn invalid_env_values_do_not_override_defaults() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _summary_format = ScopedEnv::set("TAPCUE_SUMMARY_FORMAT", "bogus");
        let _max_fail = ScopedEnv::set("TAPCUE_MAX_FAILURE_NOTIFICATIONS", "abc");
        let _format = ScopedEnv::set("TAPCUE_FORMAT", "yaml");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();

        assert_eq!(cfg.summary_format, SummaryFormat::None);
        assert_eq!(cfg.max_failure_notifications, None);
        assert_eq!(cfg.input_format, InputFormat::Auto);
    }

    #[test]
    fn env_bool_aliases_are_accepted() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _quiet = ScopedEnv::set("TAPCUE_QUIET_PARSE_ERRORS", "yes");
        let _notify_enabled = ScopedEnv::set("TAPCUE_NOTIFICATIONS_ENABLED", "off");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();

        assert!(cfg.quiet_parse_errors);
        assert!(cfg.no_notify);
    }

    #[test]
    fn invalid_bool_env_value_is_ignored() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let _quiet = ScopedEnv::set("TAPCUE_QUIET_PARSE_ERRORS", "perhaps");

        let mut cfg = EffectiveConfig::default();
        cfg.merge_env();

        assert!(!cfg.quiet_parse_errors);
    }

    #[test]
    fn effective_config_renders_user_facing_shape() {
        let cfg = EffectiveConfig {
            quiet_parse_errors: true,
            no_notify: false,
            desktop_mode: DesktopMode::ForceOn,
            input_format: InputFormat::Json,
            summary_format: SummaryFormat::Json,
            summary_file: Some(PathBuf::from("out.json")),
            dedup_failures: true,
            max_failure_notifications: Some(10),
            trace_detection: false,
        };

        let rendered = cfg.to_pretty_toml().expect("render should succeed");
        assert!(rendered.contains("[parser]"));
        assert!(rendered.contains("quiet_parse_errors = true"));
        assert!(rendered.contains("[input]"));
        assert!(rendered.contains("format = \"json\""));
        assert!(rendered.contains("[notifications]"));
        assert!(rendered.contains("dedup_failures = true"));
        assert!(rendered.contains("max_failure_notifications = 10"));
        assert!(rendered.contains("[output]"));
        assert!(rendered.contains("summary_format = \"json\""));
        assert!(rendered.contains("summary_file = \"out.json\""));
        assert!(rendered.contains("enabled = true"));
        assert!(rendered.contains("desktop = \"force-on\""));
    }

    #[test]
    fn notification_source_tracking_follows_precedence() {
        let _guard = env_lock().lock().expect("env lock should not be poisoned");
        let dir = tempdir().expect("temp dir should create");
        let path = dir.path().join("config.toml");
        fs::write(&path, "[notifications]\nenabled = false\ndesktop = \"force-on\"\n")
            .expect("config file should write");

        let _env_notify = ScopedEnv::set("TAPCUE_NO_NOTIFY", "false");
        let _env_desktop = ScopedEnv::set("TAPCUE_DESKTOP", "force-off");

        let cli = Cli {
            quiet_parse_errors: false,
            no_quiet_parse_errors: false,
            no_notify: true,
            notify: false,
            desktop: Some(crate::cli::CliDesktopMode::Auto),
            format: None,
            summary_format: None,
            summary_file: None,
            dedup_failures: false,
            no_dedup_failures: false,
            max_failure_notifications: None,
            trace_detection: false,
            validate_config: false,
            print_effective_config: false,
            doctor: false,
        };

        let mut cfg = EffectiveConfig::default();
        let mut sources = NotificationConfigSources::default();
        cfg.merge_file_internal(&path, Some(ConfigSource::UserConfig), Some(&mut sources))
            .expect("config should parse");
        cfg.merge_env_internal(Some(&mut sources));
        cfg.merge_cli_internal(&cli, Some(&mut sources));

        assert!(cfg.no_notify);
        assert_eq!(cfg.desktop_mode, DesktopMode::Auto);
        assert_eq!(sources.enabled, ConfigSource::Cli);
        assert_eq!(sources.desktop, ConfigSource::Cli);
    }
}
