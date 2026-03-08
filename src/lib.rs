//! `tapcue` consumes streaming TAP output and emits notifications for failures,
//! bailouts, and final summary status.
//!
//! Runtime behavior is controlled through layered configuration with this
//! precedence order:
//! CLI flags > environment variables > local config > user config > defaults.

pub mod cli;
pub mod config;
pub mod json_stream;
pub(crate) mod line_buffer;
pub mod notifier;
pub mod processor;

use std::io::Read;
use std::io::{BufRead, BufReader};

use anyhow::Result;

use crate::config::InputFormat;
use crate::json_stream::JsonStreamProcessor;
use crate::notifier::Notifier;
use crate::processor::{RunState, TapStreamProcessor};

const MAX_AUTO_DETECTION_BUFFER: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct AppConfig {
    pub quiet_parse_errors: bool,
    pub input_format: InputFormat,
    pub trace_detection: bool,
}

enum StreamProcessor {
    Tap(TapStreamProcessor),
    Json(JsonStreamProcessor),
}

pub fn process_stream<R: Read>(
    reader: R,
    notifier: &mut dyn Notifier,
    config: AppConfig,
) -> Result<RunState> {
    let mut buffered_reader = BufReader::with_capacity(16 * 1024, reader);
    let mut line_buf = Vec::with_capacity(256);
    let mut selected = match config.input_format {
        InputFormat::Tap => {
            Some(StreamProcessor::Tap(TapStreamProcessor::new(config.quiet_parse_errors)))
        }
        InputFormat::Json => {
            Some(StreamProcessor::Json(JsonStreamProcessor::new(config.quiet_parse_errors)))
        }
        InputFormat::Auto => None,
    };
    let mut undecided_buffer = String::new();

    loop {
        line_buf.clear();
        let bytes_read = buffered_reader.read_until(b'\n', &mut line_buf)?;
        if bytes_read == 0 {
            break;
        }

        let line = std::str::from_utf8(&line_buf)?;

        if let Some(processor) = selected.as_mut() {
            ingest_with_processor(processor, line, notifier);
            continue;
        }

        undecided_buffer.push_str(line);
        if let Some(format) = detect_auto_format(&undecided_buffer) {
            if config.trace_detection {
                eprintln!("tapcue: auto-detected input format: {}", format.as_str());
            }
            let mut processor = match format {
                InputFormat::Tap => {
                    StreamProcessor::Tap(TapStreamProcessor::new(config.quiet_parse_errors))
                }
                InputFormat::Json => {
                    StreamProcessor::Json(JsonStreamProcessor::new(config.quiet_parse_errors))
                }
                InputFormat::Auto => unreachable!(),
            };

            ingest_with_processor(&mut processor, &undecided_buffer, notifier);
            undecided_buffer.clear();
            selected = Some(processor);
        } else if undecided_buffer.len() > MAX_AUTO_DETECTION_BUFFER {
            let fallback = fallback_format(&undecided_buffer);
            if config.trace_detection {
                eprintln!(
                    "tapcue: auto-detection buffer limit reached; using fallback format: {}",
                    fallback.as_str()
                );
            }

            let mut processor = match fallback {
                InputFormat::Tap => {
                    StreamProcessor::Tap(TapStreamProcessor::new(config.quiet_parse_errors))
                }
                InputFormat::Json => {
                    StreamProcessor::Json(JsonStreamProcessor::new(config.quiet_parse_errors))
                }
                InputFormat::Auto => unreachable!(),
            };
            ingest_with_processor(&mut processor, &undecided_buffer, notifier);
            undecided_buffer.clear();
            selected = Some(processor);
        }
    }

    let mut processor = selected.unwrap_or_else(|| {
        let default_format = fallback_format(&undecided_buffer);

        if config.trace_detection {
            eprintln!("tapcue: auto-selected fallback input format: {}", default_format.as_str());
        }

        match default_format {
            InputFormat::Tap => {
                StreamProcessor::Tap(TapStreamProcessor::new(config.quiet_parse_errors))
            }
            InputFormat::Json => {
                StreamProcessor::Json(JsonStreamProcessor::new(config.quiet_parse_errors))
            }
            InputFormat::Auto => unreachable!(),
        }
    });

    if !undecided_buffer.is_empty() {
        ingest_with_processor(&mut processor, &undecided_buffer, notifier);
    }

    Ok(finish_with_processor(processor, notifier))
}

fn fallback_format(buffer: &str) -> InputFormat {
    if buffer.trim_start().starts_with('{') || buffer.trim_start().starts_with('[') {
        InputFormat::Json
    } else {
        InputFormat::Tap
    }
}

fn ingest_with_processor(
    processor: &mut StreamProcessor,
    input: &str,
    notifier: &mut dyn Notifier,
) {
    match processor {
        StreamProcessor::Tap(inner) => inner.ingest(input, notifier),
        StreamProcessor::Json(inner) => inner.ingest(input, notifier),
    }
}

fn finish_with_processor(processor: StreamProcessor, notifier: &mut dyn Notifier) -> RunState {
    match processor {
        StreamProcessor::Tap(mut inner) => {
            inner.finish(notifier);
            inner.into_state()
        }
        StreamProcessor::Json(mut inner) => {
            inner.finish(notifier);
            inner.into_state()
        }
    }
}

fn detect_auto_format(buffer: &str) -> Option<InputFormat> {
    for line in buffer.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return Some(InputFormat::Json);
        }

        if trimmed.starts_with("TAP version")
            || trimmed.starts_with("ok ")
            || trimmed.starts_with("not ok ")
            || trimmed.starts_with("Bail out!")
            || is_tap_plan_line(trimmed)
        {
            return Some(InputFormat::Tap);
        }
    }

    None
}

fn is_tap_plan_line(line: &str) -> bool {
    if let Some((left, right)) = line.split_once("..") {
        return left.parse::<usize>().is_ok() && right.parse::<usize>().is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use crate::notifier::Notifier;

    use super::{detect_auto_format, fallback_format, process_stream, AppConfig, RunState};

    #[derive(Default)]
    struct RecordingNotifier {
        failures: usize,
        summaries: usize,
    }

    impl Notifier for RecordingNotifier {
        fn notify_failure(&mut self, _label: &str) {
            self.failures += 1;
        }

        fn notify_bailout(&mut self, _reason: &str) {}

        fn notify_summary(&mut self, _state: &RunState) {
            self.summaries += 1;
        }
    }

    struct TinyChunkReader {
        data: Vec<u8>,
        cursor: usize,
        max_chunk: usize,
    }

    impl TinyChunkReader {
        fn new(input: &str, max_chunk: usize) -> Self {
            Self { data: input.as_bytes().to_vec(), cursor: 0, max_chunk }
        }
    }

    impl Read for TinyChunkReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.cursor >= self.data.len() {
                return Ok(0);
            }

            let chunk_len =
                self.max_chunk.min(buf.len()).min(self.data.len().saturating_sub(self.cursor));
            let end = self.cursor + chunk_len;
            buf[..chunk_len].copy_from_slice(&self.data[self.cursor..end]);
            self.cursor = end;
            Ok(chunk_len)
        }
    }

    #[test]
    fn process_stream_handles_utf8_split_across_read_boundaries() {
        let input = "TAP version 14\n1..1\nnot ok 1 - caf\u{e9}\n";
        let mut notifier = RecordingNotifier::default();

        let state =
            process_stream(TinyChunkReader::new(input, 1), &mut notifier, AppConfig::default())
                .expect("stream should parse");

        assert_eq!(state.failed, 1);
        assert_eq!(notifier.failures, 1);
        assert_eq!(notifier.summaries, 1);
    }

    #[test]
    fn process_stream_handles_eof_without_trailing_newline() {
        let input = "TAP version 14\n1..1\nok 1 - last";
        let mut notifier = RecordingNotifier::default();

        let state =
            process_stream(TinyChunkReader::new(input, 2), &mut notifier, AppConfig::default())
                .expect("stream should parse");

        assert_eq!(state.total, 1);
        assert_eq!(state.failed, 0);
        assert!(state.is_success());
        assert_eq!(notifier.summaries, 1);
    }

    #[test]
    fn process_stream_handles_large_tiny_chunk_input() {
        let mut input = String::from("TAP version 14\n1..2000\n");
        for index in 1..=2000 {
            input.push_str(&format!("ok {index} - case {index}\n"));
        }

        let mut notifier = RecordingNotifier::default();
        let state =
            process_stream(TinyChunkReader::new(&input, 1), &mut notifier, AppConfig::default())
                .expect("stream should parse");

        assert_eq!(state.total, 2000);
        assert_eq!(state.failed, 0);
        assert!(state.is_success());
    }

    #[test]
    fn auto_detects_json_stream() {
        let input = "{\"Action\":\"pass\",\"Test\":\"A\"}\n{\"Action\":\"fail\",\"Test\":\"B\"}\n";
        let mut notifier = RecordingNotifier::default();

        let state = process_stream(
            TinyChunkReader::new(input, 2),
            &mut notifier,
            AppConfig {
                quiet_parse_errors: false,
                input_format: crate::config::InputFormat::Auto,
                trace_detection: false,
            },
        )
        .expect("auto JSON should parse");

        assert_eq!(state.total, 2);
        assert_eq!(state.failed, 1);
    }

    #[test]
    fn explicit_tap_mode_parses_tap_without_detection() {
        let input = "TAP version 14\n1..1\nok 1 - only\n";
        let mut notifier = RecordingNotifier::default();

        let state = process_stream(
            TinyChunkReader::new(input, 3),
            &mut notifier,
            AppConfig {
                quiet_parse_errors: false,
                input_format: crate::config::InputFormat::Tap,
                trace_detection: true,
            },
        )
        .expect("tap mode should parse");

        assert_eq!(state.total, 1);
        assert!(state.is_success());
    }

    #[test]
    fn auto_fallback_kicks_in_for_large_undecided_prefix() {
        let mut input = String::new();
        input.push_str(&"x".repeat(70_000));
        input.push('\n');
        input.push_str("TAP version 14\n1..1\nok 1 - late\n");

        let mut notifier = RecordingNotifier::default();
        let state = process_stream(
            TinyChunkReader::new(&input, 16),
            &mut notifier,
            AppConfig {
                quiet_parse_errors: false,
                input_format: crate::config::InputFormat::Auto,
                trace_detection: true,
            },
        )
        .expect("fallback should parse");

        assert_eq!(state.total, 1);
        assert!(state.parse_warning_count >= 1);
    }

    #[test]
    fn auto_detection_helpers_cover_common_inputs() {
        assert!(matches!(
            detect_auto_format("{\"a\":1}\n"),
            Some(crate::config::InputFormat::Json)
        ));
        assert!(matches!(
            detect_auto_format("TAP version 14\n"),
            Some(crate::config::InputFormat::Tap)
        ));
        assert!(matches!(
            detect_auto_format("ok 1 - works\n"),
            Some(crate::config::InputFormat::Tap)
        ));
        assert!(matches!(
            detect_auto_format("not ok 1 - fails\n"),
            Some(crate::config::InputFormat::Tap)
        ));
        assert!(matches!(
            detect_auto_format("Bail out! stop\n"),
            Some(crate::config::InputFormat::Tap)
        ));
        assert!(matches!(detect_auto_format("1..5\n"), Some(crate::config::InputFormat::Tap)));
        assert!(detect_auto_format("hello world\n").is_none());
        assert!(detect_auto_format("\n\n").is_none());

        assert!(matches!(fallback_format(" [1,2,3]"), crate::config::InputFormat::Json));
        assert!(matches!(fallback_format("ok 1 - test"), crate::config::InputFormat::Tap));
        assert!(matches!(fallback_format(""), crate::config::InputFormat::Tap));
    }

    #[test]
    fn auto_mode_with_empty_input_defaults_to_tap() {
        let mut notifier = RecordingNotifier::default();
        let state = process_stream(
            TinyChunkReader::new("", 8),
            &mut notifier,
            AppConfig {
                quiet_parse_errors: false,
                input_format: crate::config::InputFormat::Auto,
                trace_detection: false,
            },
        )
        .expect("empty stream should be processed");

        assert_eq!(state.total, 0);
        assert_eq!(notifier.summaries, 1);
    }
}
