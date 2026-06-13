use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::SystemTime;

use anyhow::{Result, bail};
use tapcue::cli::RunCli;
use tapcue::config::{EffectiveConfig, RunOutputMode};
use tapcue::notifier::Notifier;
use tapcue::processor::RunState;
use tapcue::{AppConfig, process_stream};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedRunCommand {
    pub(crate) command: Vec<String>,
    pub(crate) env_overrides: Vec<(String, String)>,
    pub(crate) inferred_junit_files: Vec<PathBuf>,
    pub(crate) inferred_runner: Option<InferredJunitRunner>,
    pub(crate) started_at: SystemTime,
}

pub(crate) fn resolve_run_command(
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

    let executable = executable_name(program);

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
    let program = command.first()?;
    let executable = executable_name(program);

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

pub(crate) fn run_and_wait(run_command: &ResolvedRunCommand) -> Result<std::process::ExitStatus> {
    let mut command = build_run_command(run_command)?;

    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    Ok(status)
}

pub(crate) fn run_and_process(
    run_command: &ResolvedRunCommand,
    notifier: &mut dyn Notifier,
    app_config: AppConfig,
    run_output: RunOutputMode,
) -> Result<(RunState, std::process::ExitStatus)> {
    let mut command = build_run_command(run_command)?;

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

fn build_run_command(run_command: &ResolvedRunCommand) -> Result<Command> {
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

    Ok(command)
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

pub(crate) fn infer_junit_globs_for_command(
    command: &[String],
) -> (Option<InferredJunitRunner>, Vec<String>) {
    let Some(program) = command.first() else {
        return (None, Vec::new());
    };

    let executable = executable_name(program);

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

fn executable_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum InferredJunitRunner {
    Gradle,
    Maven,
    Pytest,
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use tapcue::cli::RunCli;
    use tapcue::config::{DesktopMode, EffectiveConfig, InputFormat, RunOutputMode, SummaryFormat};

    use super::resolve_run_command;

    fn sample_config() -> EffectiveConfig {
        EffectiveConfig {
            quiet_parse_errors: false,
            strict: false,
            no_notify: true,
            desktop_mode: DesktopMode::Auto,
            include_project_context: true,
            project_label: None,
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
