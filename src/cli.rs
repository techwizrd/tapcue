use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};

use crate::config::{DesktopMode, InputFormat, RunOutputMode, SummaryFormat};

#[derive(Debug, Parser)]
#[command(author, version, about = "Emit desktop notifications from TAP stream")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,

    #[arg(
        long = "quiet-parse-errors",
        action = ArgAction::SetTrue,
        conflicts_with = "no_quiet_parse_errors",
        help = "Suppress parse warnings for malformed TAP"
    )]
    pub quiet_parse_errors: bool,

    #[arg(
        long = "strict",
        action = ArgAction::SetTrue,
        help = "Enforce TAP14 strict mode regardless of pragma"
    )]
    pub strict: bool,

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

    #[arg(long, value_name = "PATH", help = "Ingest JUnit XML report file", action = ArgAction::Append)]
    pub junit_file: Vec<String>,

    #[arg(long, value_name = "DIR", help = "Ingest all JUnit XML reports under directory", action = ArgAction::Append)]
    pub junit_dir: Vec<String>,

    #[arg(long, value_name = "GLOB", help = "Ingest JUnit XML reports matching glob", action = ArgAction::Append)]
    pub junit_glob: Vec<String>,

    #[arg(long, default_value_t = false, help = "Skip stdin parsing and use only JUnit reports")]
    pub junit_only: bool,

    #[arg(long, value_enum, help = "Runner output passthrough in run mode")]
    pub run_output: Option<CliRunOutputMode>,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "no_auto_junit_reports",
        help = "Auto-discover common JUnit report paths in run mode"
    )]
    pub auto_junit_reports: bool,

    #[arg(
        long,
        action = ArgAction::SetTrue,
        conflicts_with = "auto_junit_reports",
        help = "Disable run-mode JUnit report auto-discovery"
    )]
    pub no_auto_junit_reports: bool,

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

impl Cli {
    pub fn without_overrides() -> Self {
        Self {
            command: None,
            quiet_parse_errors: false,
            strict: false,
            no_quiet_parse_errors: false,
            no_notify: false,
            notify: false,
            desktop: None,
            format: None,
            summary_format: None,
            summary_file: None,
            junit_file: Vec::new(),
            junit_dir: Vec::new(),
            junit_glob: Vec::new(),
            junit_only: false,
            run_output: None,
            auto_junit_reports: false,
            no_auto_junit_reports: false,
            dedup_failures: false,
            no_dedup_failures: false,
            max_failure_notifications: None,
            trace_detection: false,
            validate_config: false,
            print_effective_config: false,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    Init(InitCli),
    Doctor,
    Run(RunCli),
}

#[derive(Debug, Args)]
pub struct InitCli {
    #[arg(long, default_value_t = false, help = "Write current effective config")]
    pub current: bool,

    #[arg(long, default_value_t = false, help = "Overwrite existing .tapcue.toml")]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RunCli {
    #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
    pub command: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CliDesktopMode {
    Auto,
    ForceOn,
    ForceOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CliInputFormat {
    Auto,
    Tap,
    Json,
    Bun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CliSummaryFormat {
    None,
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum CliRunOutputMode {
    Split,
    Merged,
    Off,
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
            CliInputFormat::Bun => InputFormat::Bun,
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

impl From<CliRunOutputMode> for RunOutputMode {
    fn from(value: CliRunOutputMode) -> Self {
        match value {
            CliRunOutputMode::Split => RunOutputMode::Split,
            CliRunOutputMode::Merged => RunOutputMode::Merged,
            CliRunOutputMode::Off => RunOutputMode::Off,
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        Cli, CliCommand, CliDesktopMode, CliInputFormat, CliRunOutputMode, CliSummaryFormat,
    };
    use crate::config::{DesktopMode, InputFormat, RunOutputMode, SummaryFormat};

    #[test]
    fn parses_flags_and_options() {
        let cli = Cli::parse_from([
            "tapcue",
            "--quiet-parse-errors",
            "--strict",
            "--notify",
            "--desktop",
            "force-on",
            "--format",
            "json",
            "--summary-format",
            "text",
            "--summary-file",
            "-",
            "--junit-file",
            "report.xml",
            "--junit-dir",
            "build/test-results",
            "--junit-glob",
            "**/junit/*.xml",
            "--junit-only",
            "--run-output",
            "merged",
            "--no-auto-junit-reports",
            "--dedup-failures",
            "--max-failure-notifications",
            "12",
            "--trace-detection",
            "--validate-config",
            "--print-effective-config",
        ]);

        assert!(cli.quiet_parse_errors);
        assert!(cli.strict);
        assert!(cli.notify);
        assert_eq!(cli.desktop, Some(CliDesktopMode::ForceOn));
        assert_eq!(cli.format, Some(CliInputFormat::Json));
        assert_eq!(cli.summary_format, Some(CliSummaryFormat::Text));
        assert_eq!(cli.summary_file.as_deref(), Some("-"));
        assert_eq!(cli.junit_file, vec!["report.xml"]);
        assert_eq!(cli.junit_dir, vec!["build/test-results"]);
        assert_eq!(cli.junit_glob, vec!["**/junit/*.xml"]);
        assert!(cli.junit_only);
        assert_eq!(cli.run_output, Some(CliRunOutputMode::Merged));
        assert!(cli.no_auto_junit_reports);
        assert!(cli.dedup_failures);
        assert_eq!(cli.max_failure_notifications, Some(12));
        assert!(cli.trace_detection);
        assert!(cli.validate_config);
        assert!(cli.print_effective_config);
        assert!(cli.command.is_none());
    }

    #[test]
    fn parses_init_subcommand() {
        let cli = Cli::parse_from(["tapcue", "init", "--current", "--force"]);
        match cli.command {
            Some(CliCommand::Init(init)) => {
                assert!(init.current);
                assert!(init.force);
            }
            _ => panic!("expected init subcommand"),
        }
    }

    #[test]
    fn parses_doctor_subcommand() {
        let cli = Cli::parse_from(["tapcue", "doctor"]);
        assert!(matches!(cli.command, Some(CliCommand::Doctor)));
    }

    #[test]
    fn parses_run_subcommand_command_args() {
        let cli = Cli::parse_from(["tapcue", "run", "--", "bun", "test", "--watch"]);
        match cli.command {
            Some(CliCommand::Run(run)) => {
                assert_eq!(run.command, vec!["bun", "test", "--watch"]);
            }
            _ => panic!("expected run subcommand"),
        }
    }

    #[test]
    fn enum_conversions_cover_all_values() {
        assert!(matches!(DesktopMode::from(CliDesktopMode::Auto), DesktopMode::Auto));
        assert!(matches!(DesktopMode::from(CliDesktopMode::ForceOn), DesktopMode::ForceOn));
        assert!(matches!(DesktopMode::from(CliDesktopMode::ForceOff), DesktopMode::ForceOff));

        assert!(matches!(InputFormat::from(CliInputFormat::Auto), InputFormat::Auto));
        assert!(matches!(InputFormat::from(CliInputFormat::Tap), InputFormat::Tap));
        assert!(matches!(InputFormat::from(CliInputFormat::Json), InputFormat::Json));
        assert!(matches!(InputFormat::from(CliInputFormat::Bun), InputFormat::Bun));

        assert!(matches!(SummaryFormat::from(CliSummaryFormat::None), SummaryFormat::None));
        assert!(matches!(SummaryFormat::from(CliSummaryFormat::Text), SummaryFormat::Text));
        assert!(matches!(SummaryFormat::from(CliSummaryFormat::Json), SummaryFormat::Json));

        assert!(matches!(RunOutputMode::from(CliRunOutputMode::Split), RunOutputMode::Split));
        assert!(matches!(RunOutputMode::from(CliRunOutputMode::Merged), RunOutputMode::Merged));
        assert!(matches!(RunOutputMode::from(CliRunOutputMode::Off), RunOutputMode::Off));
    }
}
