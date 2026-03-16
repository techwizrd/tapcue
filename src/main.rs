use std::fs;
use std::io;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Result};
use clap::Parser;

use tapcue::cli::{Cli, CliCommand, InitCli, RunCli};
use tapcue::config::{
    resolved_config_paths, EffectiveConfig, InputFormat, NotificationConfigSources, RunOutputMode,
    SummaryFormat,
};
use tapcue::junit_reports::ingest_junit_file;
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
        Box::new(DesktopNotifier::new(effective_config.desktop_mode))
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
                    (empty_state(), status, false)
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
                empty_state()
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
        state = empty_state();
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

    if state.is_success() {
        Ok(())
    } else {
        std::process::exit(1);
    }
}

#[derive(Clone, Debug)]
struct ResolvedRunCommand {
    command: Vec<String>,
    env_overrides: Vec<(String, String)>,
    inferred_junit_files: Vec<PathBuf>,
    inferred_runner: Option<InferredJunitRunner>,
    started_at: SystemTime,
}

fn resolve_run_command(
    run_cli: &RunCli,
    config: &EffectiveConfig,
    started_at: SystemTime,
) -> Result<ResolvedRunCommand> {
    let mut resolved = ResolvedRunCommand {
        command: run_cli.command.clone(),
        env_overrides: Vec::new(),
        inferred_junit_files: Vec::new(),
        inferred_runner: None,
        started_at,
    };

    if !config.auto_runner_adapt {
        return Ok(resolved);
    }

    let Some(program) = resolved.command.first() else {
        bail!("tapcue: run command is required");
    };

    let executable = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if executable == "go"
        && resolved.command.get(1).is_some_and(|arg| arg == "test")
        && !resolved.command.iter().any(|arg| arg == "-json" || arg == "--json")
    {
        resolved.command.insert(2, "-json".to_owned());
        return Ok(resolved);
    }

    if executable == "cargo"
        && resolved.command.get(1).is_some_and(|arg| arg == "nextest")
        && resolved.command.get(2).is_some_and(|arg| arg == "run")
    {
        if !resolved
            .command
            .iter()
            .any(|arg| arg == "--message-format" || arg.starts_with("--message-format="))
        {
            insert_before_runner_passthrough(
                &mut resolved.command,
                vec!["--message-format".to_owned(), "libtest-json-plus".to_owned()],
            );
        }

        if std::env::var_os("NEXTEST_EXPERIMENTAL_LIBTEST_JSON").is_none() {
            resolved
                .env_overrides
                .push(("NEXTEST_EXPERIMENTAL_LIBTEST_JSON".to_owned(), "1".to_owned()));
        }

        return Ok(resolved);
    }

    if executable == "jest" {
        if !resolved.command.iter().any(|arg| arg == "--json") {
            insert_before_runner_passthrough(&mut resolved.command, vec!["--json".to_owned()]);
        }

        if !resolved
            .command
            .iter()
            .any(|arg| arg == "--outputFile" || arg.starts_with("--outputFile="))
        {
            insert_before_runner_passthrough(
                &mut resolved.command,
                vec!["--outputFile".to_owned(), "/dev/stdout".to_owned()],
            );
        }

        return Ok(resolved);
    }

    if executable == "vitest" {
        if !resolved.command.iter().any(|arg| arg == "--reporter" || arg.starts_with("--reporter="))
        {
            insert_before_runner_passthrough(
                &mut resolved.command,
                vec!["--reporter=json".to_owned()],
            );
        }

        return Ok(resolved);
    }

    if let Some(pytest_insert_at) = pytest_argument_insert_index(&resolved.command) {
        let has_junitxml = resolved
            .command
            .iter()
            .any(|arg| arg == "--junitxml" || arg.starts_with("--junitxml="));
        if !has_junitxml {
            let report = generated_pytest_report_path(started_at);
            resolved.command.splice(
                pytest_insert_at..pytest_insert_at,
                ["--junitxml".to_owned(), report.to_string_lossy().to_string()],
            );
            resolved.inferred_junit_files.push(report);
            resolved.inferred_runner = Some(InferredJunitRunner::Pytest);
            return Ok(resolved);
        }
    }

    Ok(resolved)
}

fn insert_before_runner_passthrough(command: &mut Vec<String>, injected_args: Vec<String>) {
    let insert_at = command.iter().position(|arg| arg == "--").unwrap_or(command.len());
    command.splice(insert_at..insert_at, injected_args);
}

fn generated_pytest_report_path(started_at: SystemTime) -> PathBuf {
    let nanos = started_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("tapcue-pytest-{nanos}-{}.xml", std::process::id()))
}

fn pytest_argument_insert_index(command: &[String]) -> Option<usize> {
    let Some(program) = command.first() else {
        return None;
    };

    let executable = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if executable == "pytest" {
        return Some(1);
    }

    if executable == "python" || executable.starts_with("python3") {
        for (index, window) in command.windows(2).enumerate() {
            if window[0] == "-m" && window[1] == "pytest" {
                return Some(index + 2);
            }
        }
        return None;
    }

    if (executable == "uv" || executable == "poetry")
        && command.get(1).is_some_and(|arg| arg == "run")
    {
        let mut index = 2;
        if command.get(index).is_some_and(|arg| arg == "--") {
            index += 1;
        }

        if command.get(index).is_some_and(|arg| arg == "pytest") {
            return Some(index + 1);
        }
    }

    None
}

fn run_and_wait(run_command: &ResolvedRunCommand) -> Result<std::process::ExitStatus> {
    let program = run_command
        .command
        .first()
        .ok_or_else(|| anyhow::anyhow!("tapcue: run command is required"))?;
    let args = &run_command.command[1..];

    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in &run_command.env_overrides {
        command.env(key, value);
    }

    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    Ok(status)
}

fn run_and_process(
    run_command: &ResolvedRunCommand,
    notifier: &mut dyn Notifier,
    app_config: AppConfig,
    run_output: RunOutputMode,
) -> Result<(RunState, std::process::ExitStatus)> {
    let program = run_command
        .command
        .first()
        .ok_or_else(|| anyhow::anyhow!("tapcue: run command is required"))?;
    let args = &run_command.command[1..];

    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in &run_command.env_overrides {
        command.env(key, value);
    }

    let mut child =
        command.stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("tapcue: failed to capture child stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("tapcue: failed to capture child stderr"))?;

    let merged = MergedReader::new(stdout, stderr, run_output);
    let state = process_stream(merged, notifier, app_config)?;
    let status = child.wait()?;

    Ok((state, status))
}

struct MergedReader {
    rx: Receiver<StreamChunk>,
    current: std::io::Cursor<Vec<u8>>,
    run_output: RunOutputMode,
}

impl MergedReader {
    fn new(
        stdout: std::process::ChildStdout,
        stderr: std::process::ChildStderr,
        run_output: RunOutputMode,
    ) -> Self {
        let (tx, rx) = mpsc::channel::<StreamChunk>();

        let stdout_tx = tx.clone();
        thread::spawn(move || pump_reader(stdout, StreamSource::Stdout, stdout_tx));

        let stderr_tx = tx.clone();
        thread::spawn(move || pump_reader(stderr, StreamSource::Stderr, stderr_tx));

        drop(tx);

        Self { rx, current: std::io::Cursor::new(Vec::new()), run_output }
    }
}

impl Read for MergedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let current = self.current.get_ref();
            let position = self.current.position() as usize;
            if position < current.len() {
                return self.current.read(buf);
            }

            match self.rx.recv() {
                Ok(chunk) => {
                    self.mirror_chunk(&chunk)?;
                    self.current = std::io::Cursor::new(chunk.bytes);
                }
                Err(_) => return Ok(0),
            }
        }
    }
}

impl MergedReader {
    fn mirror_chunk(&self, chunk: &StreamChunk) -> io::Result<()> {
        match self.run_output {
            RunOutputMode::Off => Ok(()),
            RunOutputMode::Split => match chunk.source {
                StreamSource::Stdout => io::stdout().write_all(&chunk.bytes),
                StreamSource::Stderr => io::stderr().write_all(&chunk.bytes),
            },
            RunOutputMode::Merged => io::stdout().write_all(&chunk.bytes),
        }
    }
}

fn pump_reader<R: Read + Send + 'static>(
    mut reader: R,
    source: StreamSource,
    tx: mpsc::Sender<StreamChunk>,
) {
    let mut buffer = [0_u8; 8192];

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let chunk = StreamChunk { source, bytes: buffer[..read].to_vec() };
                if tx.send(chunk).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

#[derive(Clone, Copy)]
enum StreamSource {
    Stdout,
    Stderr,
}

struct StreamChunk {
    source: StreamSource,
    bytes: Vec<u8>,
}

fn resolve_junit_report_files(
    config: &EffectiveConfig,
    run_command: Option<&[String]>,
    inferred_junit_files: &[PathBuf],
    run_started_at: Option<SystemTime>,
    trace_detection: bool,
) -> Result<JunitReportResolution> {
    let mut files = Vec::new();

    files.extend_from_slice(inferred_junit_files);

    for file in &config.junit_file {
        files.push(file.clone());
    }

    for dir_path in &config.junit_dir {
        let pattern = format!("{}/**/*.xml", dir_path.display());
        for entry in glob::glob(&pattern)? {
            match entry {
                Ok(path) if path.is_file() => files.push(path),
                Ok(_) => {}
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "tapcue: invalid JUnit directory expansion for {dir}: {error}",
                        dir = dir_path.display()
                    ));
                }
            }
        }
    }

    for pattern in &config.junit_glob {
        for entry in glob::glob(pattern)? {
            match entry {
                Ok(path) if path.is_file() => files.push(path),
                Ok(_) => {}
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "tapcue: invalid JUnit glob match for {pattern}: {error}"
                    ));
                }
            }
        }
    }

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
                for path in glob::glob(&pattern)?.flatten() {
                    if path.is_file() {
                        files.push(path);
                    }
                }
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

fn is_fresh_report(modified: SystemTime, started_at: SystemTime) -> bool {
    const MTIME_TOLERANCE: Duration = Duration::from_secs(2);
    match started_at.checked_sub(MTIME_TOLERANCE) {
        Some(threshold) => modified >= threshold,
        None => true,
    }
}

fn infer_junit_globs_for_command(command: &[String]) -> (Option<InferredJunitRunner>, Vec<String>) {
    let Some(program) = command.first() else {
        return (None, Vec::new());
    };

    let executable = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    if executable == "gradle" || executable == "gradlew" {
        return (
            Some(InferredJunitRunner::Gradle),
            vec!["**/build/test-results/**/*.xml".to_owned()],
        );
    }

    if executable == "mvn" || executable == "mvnw" {
        return (
            Some(InferredJunitRunner::Maven),
            vec![
                "**/target/surefire-reports/TEST-*.xml".to_owned(),
                "**/target/failsafe-reports/TEST-*.xml".to_owned(),
            ],
        );
    }

    (None, Vec::new())
}

fn should_prefer_inferred_junit(
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

struct JunitReportResolution {
    files: Vec<PathBuf>,
    inferred_runner: Option<InferredJunitRunner>,
    matched_existing_but_unmodified: bool,
}

#[derive(Clone, Copy, Debug)]
enum InferredJunitRunner {
    Gradle,
    Maven,
    Pytest,
}

fn ingest_junit_reports(
    junit_files: &[PathBuf],
    notifier: &mut dyn Notifier,
    quiet_parse_errors: bool,
    trace_detection: bool,
) -> RunState {
    let mut state = empty_state();

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

fn merge_run_state(state: &mut RunState, add: &RunState) {
    if add.total > 0 {
        state.planned = None;
    }

    state.total += add.total;
    state.passed += add.passed;
    state.failed += add.failed;
    state.todo += add.todo;
    state.skipped += add.skipped;
    state.parse_warning_count += add.parse_warning_count;
    state.protocol_failures += add.protocol_failures;

    if state.bailout_reason.is_none() {
        state.bailout_reason = add.bailout_reason.clone();
    }
}

fn empty_state() -> RunState {
    RunState {
        planned: None,
        total: 0,
        passed: 0,
        failed: 0,
        todo: 0,
        skipped: 0,
        bailout_reason: None,
        parse_warning_count: 0,
        protocol_failures: 0,
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
    use std::time::{Duration, SystemTime};

    use tempfile::tempdir;

    use super::{
        emit_summary, is_fresh_report, render_json_summary, render_text_summary,
        resolve_run_command, summary_destination,
    };
    use tapcue::cli::RunCli;
    use tapcue::config::{DesktopMode, EffectiveConfig, InputFormat, RunOutputMode, SummaryFormat};
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
            run_output: RunOutputMode::Split,
            auto_runner_adapt: true,
            junit_file: Vec::new(),
            junit_dir: Vec::new(),
            junit_glob: Vec::new(),
            junit_only: false,
            auto_junit_reports: true,
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

    #[test]
    fn fresh_report_tolerates_small_mtime_skew() {
        let started = SystemTime::now();
        let modified = started - Duration::from_secs(1);
        assert!(is_fresh_report(modified, started));

        let old_modified = started - Duration::from_secs(5);
        assert!(!is_fresh_report(old_modified, started));
    }

    #[test]
    fn run_auto_adapts_go_test_to_json() {
        let cfg = sample_config();
        let run_cli =
            RunCli { command: vec!["go".to_owned(), "test".to_owned(), "./...".to_owned()] };
        let resolved =
            resolve_run_command(&run_cli, &cfg, SystemTime::now()).expect("command should resolve");

        assert_eq!(resolved.command, vec!["go", "test", "-json", "./..."]);
    }

    #[test]
    fn run_auto_adapts_pytest_to_junit_report() {
        let cfg = sample_config();
        let run_cli = RunCli { command: vec!["pytest".to_owned()] };
        let resolved =
            resolve_run_command(&run_cli, &cfg, SystemTime::now()).expect("command should resolve");

        assert!(resolved.command.iter().any(|arg| arg == "--junitxml"));
        assert_eq!(resolved.inferred_junit_files.len(), 1);
    }

    #[test]
    fn run_auto_adapt_can_be_disabled() {
        let mut cfg = sample_config();
        cfg.auto_runner_adapt = false;
        let run_cli =
            RunCli { command: vec!["go".to_owned(), "test".to_owned(), "./...".to_owned()] };
        let resolved =
            resolve_run_command(&run_cli, &cfg, SystemTime::now()).expect("command should resolve");

        assert_eq!(resolved.command, vec!["go", "test", "./..."]);
    }

    #[test]
    fn run_auto_adapts_uv_run_pytest_to_junit_report() {
        let cfg = sample_config();
        let run_cli =
            RunCli { command: vec!["uv".to_owned(), "run".to_owned(), "pytest".to_owned()] };
        let resolved =
            resolve_run_command(&run_cli, &cfg, SystemTime::now()).expect("command should resolve");

        assert_eq!(resolved.command[0], "uv");
        assert_eq!(resolved.command[1], "run");
        assert_eq!(resolved.command[2], "pytest");
        assert_eq!(resolved.command[3], "--junitxml");
        assert_eq!(resolved.inferred_junit_files.len(), 1);
    }

    #[test]
    fn run_auto_adapts_poetry_run_dash_dash_pytest_to_junit_report() {
        let cfg = sample_config();
        let run_cli = RunCli {
            command: vec![
                "poetry".to_owned(),
                "run".to_owned(),
                "--".to_owned(),
                "pytest".to_owned(),
            ],
        };
        let resolved =
            resolve_run_command(&run_cli, &cfg, SystemTime::now()).expect("command should resolve");

        assert_eq!(resolved.command[0], "poetry");
        assert_eq!(resolved.command[1], "run");
        assert_eq!(resolved.command[2], "--");
        assert_eq!(resolved.command[3], "pytest");
        assert_eq!(resolved.command[4], "--junitxml");
        assert_eq!(resolved.inferred_junit_files.len(), 1);
    }
}
