use proptest::prelude::*;
use tapcue::config::InputFormat;
use tapcue::json_stream::JsonStreamProcessor;
use tapcue::notifier::{FailureNotification, Notifier};
use tapcue::processor::{RunState, TapStreamProcessor};
use tapcue::{process_stream, AppConfig};

mod common;

use common::property_strategies::{
    assert_state_invariants, ingest_text_chunks, json_outcome_strategy, render_go_json_case,
    render_tap_case, tap_outcome_strategy, ChunkedReader, JsonOutcome, TapOutcome,
};

#[derive(Default)]
struct RecordingNotifier {
    failures: usize,
    summaries: usize,
}

impl Notifier for RecordingNotifier {
    fn notify_failure(&mut self, _failure: &FailureNotification) {
        self.failures += 1;
    }

    fn notify_bailout(&mut self, _reason: &str) {}

    fn notify_summary(&mut self, _state: &RunState) {
        self.summaries += 1;
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        max_local_rejects: 4096,
        .. ProptestConfig::default()
    })]

    #[test]
    fn tap_parser_respects_state_invariants_for_arbitrary_utf8ish_input(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
        chunk_sizes in prop::collection::vec(1usize..64usize, 1..16),
    ) {
        let input = String::from_utf8_lossy(&bytes).into_owned();

        let mut notifier = RecordingNotifier::default();
        let mut parser = TapStreamProcessor::new(true);

        ingest_text_chunks(&input, &chunk_sizes, |chunk| {
            parser.ingest(chunk, &mut notifier);
        });

        parser.finish(&mut notifier);
        let state = parser.into_state();

        assert_state_invariants(&state)?;
        prop_assert_eq!(notifier.summaries, 1);
    }

    #[test]
    fn json_parser_respects_state_invariants_for_arbitrary_utf8ish_input(
        bytes in prop::collection::vec(any::<u8>(), 0..4096),
        chunk_sizes in prop::collection::vec(1usize..64usize, 1..16),
    ) {
        let input = String::from_utf8_lossy(&bytes).into_owned();

        let mut notifier = RecordingNotifier::default();
        let mut parser = JsonStreamProcessor::new(true);

        ingest_text_chunks(&input, &chunk_sizes, |chunk| {
            parser.ingest(chunk, &mut notifier);
        });

        parser.finish(&mut notifier);
        let state = parser.into_state();

        assert_state_invariants(&state)?;
        prop_assert_eq!(notifier.summaries, 1);
    }

    #[test]
    fn process_stream_tap_is_chunking_invariant(
        bytes in prop::collection::vec(any::<u8>(), 0..2048),
        chunk_sizes in prop::collection::vec(1usize..32usize, 1..12),
    ) {
        let input = String::from_utf8_lossy(&bytes).into_owned();
        let config = AppConfig {
            quiet_parse_errors: true,
            input_format: InputFormat::Tap,
            trace_detection: false,
        };

        let mut whole_notifier = RecordingNotifier::default();
        let whole_state = process_stream(input.as_bytes(), &mut whole_notifier, config)
            .expect("tap processing should not fail for valid utf-8");

        let reader = ChunkedReader::new(input.into_bytes(), chunk_sizes);
        let mut chunked_notifier = RecordingNotifier::default();
        let chunked_state = process_stream(reader, &mut chunked_notifier, config)
            .expect("tap chunked processing should not fail");

        prop_assert_eq!(whole_state, chunked_state);
    }

    #[test]
    fn process_stream_json_is_chunking_invariant(
        bytes in prop::collection::vec(any::<u8>(), 0..2048),
        chunk_sizes in prop::collection::vec(1usize..32usize, 1..12),
    ) {
        let input = String::from_utf8_lossy(&bytes).into_owned();
        let config = AppConfig {
            quiet_parse_errors: true,
            input_format: InputFormat::Json,
            trace_detection: false,
        };

        let mut whole_notifier = RecordingNotifier::default();
        let whole_state = process_stream(input.as_bytes(), &mut whole_notifier, config)
            .expect("json processing should not fail for valid utf-8");

        let reader = ChunkedReader::new(input.into_bytes(), chunk_sizes);
        let mut chunked_notifier = RecordingNotifier::default();
        let chunked_state = process_stream(reader, &mut chunked_notifier, config)
            .expect("json chunked processing should not fail");

        prop_assert_eq!(whole_state, chunked_state);
    }

    #[test]
    fn generated_valid_tap_matches_expected_counts(
        outcomes in prop::collection::vec(tap_outcome_strategy(), 0..200),
    ) {
        let input = render_tap_case(&outcomes);
        let mut notifier = RecordingNotifier::default();

        let mut parser = TapStreamProcessor::new(true);
        parser.ingest(&input, &mut notifier);
        parser.finish(&mut notifier);
        let state = parser.into_state();

        let expected_passed = outcomes.iter().filter(|outcome| matches!(outcome, TapOutcome::Pass)).count();
        let expected_failed = outcomes.iter().filter(|outcome| matches!(outcome, TapOutcome::Fail)).count();
        let expected_todo = outcomes.iter().filter(|outcome| matches!(outcome, TapOutcome::Todo)).count();
        let expected_skipped = outcomes.iter().filter(|outcome| matches!(outcome, TapOutcome::Skip)).count();

        prop_assert_eq!(state.total, outcomes.len());
        prop_assert_eq!(state.passed, expected_passed);
        prop_assert_eq!(state.failed, expected_failed);
        prop_assert_eq!(state.todo, expected_todo);
        prop_assert_eq!(state.skipped, expected_skipped);
        prop_assert_eq!(state.protocol_failures, 0);
        prop_assert_eq!(state.parse_warning_count, 0);
        prop_assert_eq!(notifier.failures, expected_failed);
        prop_assert_eq!(notifier.summaries, 1);
        assert_state_invariants(&state)?;
    }

    #[test]
    fn generated_go_json_events_match_expected_counts(
        outcomes in prop::collection::vec(json_outcome_strategy(), 0..200),
    ) {
        let input = render_go_json_case(&outcomes);
        let mut notifier = RecordingNotifier::default();

        let mut parser = JsonStreamProcessor::new(true);
        parser.ingest(&input, &mut notifier);
        parser.finish(&mut notifier);
        let state = parser.into_state();

        let expected_passed = outcomes.iter().filter(|outcome| matches!(outcome, JsonOutcome::Pass)).count();
        let expected_failed = outcomes.iter().filter(|outcome| matches!(outcome, JsonOutcome::Fail)).count();
        let expected_skipped = outcomes.iter().filter(|outcome| matches!(outcome, JsonOutcome::Skip)).count();

        prop_assert_eq!(state.total, outcomes.len());
        prop_assert_eq!(state.passed, expected_passed);
        prop_assert_eq!(state.failed, expected_failed);
        prop_assert_eq!(state.skipped, expected_skipped);
        prop_assert_eq!(state.todo, 0);
        prop_assert_eq!(notifier.failures, expected_failed);
        prop_assert_eq!(notifier.summaries, 1);
        assert_state_invariants(&state)?;
    }
}
