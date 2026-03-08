use clap::{ArgAction, Parser, ValueEnum};

use crate::config::{DesktopMode, InputFormat, SummaryFormat};

#[derive(Debug, Parser)]
#[command(author, version, about = "Emit desktop notifications from TAP stream")]
pub struct Cli {
    #[arg(
        long = "quiet-parse-errors",
        action = ArgAction::SetTrue,
        conflicts_with = "no_quiet_parse_errors",
        help = "Suppress parse warnings for malformed TAP"
    )]
    pub quiet_parse_errors: bool,

    #[arg(
        long = "no-quiet-parse-errors",
        action = ArgAction::SetTrue,
        conflicts_with = "quiet_parse_errors",
        help = "Force parse warnings on"
    )]
    pub no_quiet_parse_errors: bool,

    #[arg(
        long = "no-notify",
        action = ArgAction::SetTrue,
        conflicts_with = "notify",
        help = "Disable desktop notifications (useful in CI/tests)"
    )]
    pub no_notify: bool,

    #[arg(
        long = "notify",
        action = ArgAction::SetTrue,
        conflicts_with = "no_notify",
        help = "Force desktop notifications on"
    )]
    pub notify: bool,

    #[arg(long, value_enum, help = "Desktop notification mode override")]
    pub desktop: Option<CliDesktopMode>,

    #[arg(long, value_enum, help = "Input format (default: auto)")]
    pub format: Option<CliInputFormat>,

    #[arg(long, value_enum, help = "Emit run summary as text or JSON")]
    pub summary_format: Option<CliSummaryFormat>,

    #[arg(long, help = "Write run summary to file path")]
    pub summary_file: Option<String>,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "no_dedup_failures",
        help = "Deduplicate repeated failure notifications"
    )]
    pub dedup_failures: bool,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "dedup_failures",
        help = "Disable failure notification deduplication"
    )]
    pub no_dedup_failures: bool,

    #[arg(long, help = "Cap emitted failure notifications")]
    pub max_failure_notifications: Option<usize>,

    #[arg(long, default_value_t = false, help = "Print parser format detection details")]
    pub trace_detection: bool,

    #[arg(long, default_value_t = false, help = "Validate merged config and exit")]
    pub validate_config: bool,

    #[arg(long, default_value_t = false, help = "Print effective merged configuration and exit")]
    pub print_effective_config: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CliDesktopMode {
    Auto,
    ForceOn,
    ForceOff,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CliInputFormat {
    Auto,
    Tap,
    Json,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CliSummaryFormat {
    None,
    Text,
    Json,
}

impl From<CliDesktopMode> for DesktopMode {
    fn from(value: CliDesktopMode) -> Self {
        match value {
            CliDesktopMode::Auto => DesktopMode::Auto,
            CliDesktopMode::ForceOn => DesktopMode::ForceOn,
            CliDesktopMode::ForceOff => DesktopMode::ForceOff,
        }
    }
}

impl From<CliInputFormat> for InputFormat {
    fn from(value: CliInputFormat) -> Self {
        match value {
            CliInputFormat::Auto => InputFormat::Auto,
            CliInputFormat::Tap => InputFormat::Tap,
            CliInputFormat::Json => InputFormat::Json,
        }
    }
}

impl From<CliSummaryFormat> for SummaryFormat {
    fn from(value: CliSummaryFormat) -> Self {
        match value {
            CliSummaryFormat::None => SummaryFormat::None,
            CliSummaryFormat::Text => SummaryFormat::Text,
            CliSummaryFormat::Json => SummaryFormat::Json,
        }
    }
}
