use std::io::Read;

use proptest::prelude::*;
use tapcue::processor::RunState;

#[derive(Clone, Debug)]
pub struct ChunkedReader {
    data: Vec<u8>,
    offset: usize,
    chunk_sizes: Vec<usize>,
    chunk_index: usize,
}

impl ChunkedReader {
    pub fn new(data: Vec<u8>, mut chunk_sizes: Vec<usize>) -> Self {
        if chunk_sizes.is_empty() {
            chunk_sizes.push(1);
        }
        Self { data, offset: 0, chunk_sizes, chunk_index: 0 }
    }
}

impl Read for ChunkedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.data.len() {
            return Ok(0);
        }

        let next_size = self.chunk_sizes[self.chunk_index % self.chunk_sizes.len()].max(1);
        self.chunk_index += 1;
        let count = next_size.min(buf.len()).min(self.data.len().saturating_sub(self.offset));
        let end = self.offset + count;
        buf[..count].copy_from_slice(&self.data[self.offset..end]);
        self.offset = end;
        Ok(count)
    }
}

pub fn ingest_text_chunks<F>(input: &str, chunk_sizes: &[usize], mut ingest: F)
where
    F: FnMut(&str),
{
    let mut cursor = 0;
    let mut chunk_index = 0;

    while cursor < input.len() {
        let size = chunk_sizes[chunk_index % chunk_sizes.len()].max(1);
        chunk_index += 1;

        let mut end = (cursor + size).min(input.len());
        while end > cursor && !input.is_char_boundary(end) {
            end -= 1;
        }

        if end == cursor {
            end = (cursor + 1).min(input.len());
            while end < input.len() && !input.is_char_boundary(end) {
                end += 1;
            }
        }

        ingest(&input[cursor..end]);
        cursor = end;
    }
}

pub fn assert_state_invariants(state: &RunState) -> proptest::test_runner::TestCaseResult {
    prop_assert_eq!(state.total, state.passed + state.failed + state.todo + state.skipped);
    if state.is_success() {
        prop_assert_eq!(state.failed, 0);
        prop_assert!(state.bailout_reason.is_none());
        prop_assert_eq!(state.protocol_failures, 0);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
pub enum TapOutcome {
    Pass,
    Fail,
    Todo,
    Skip,
}

pub fn tap_outcome_strategy() -> impl Strategy<Value = TapOutcome> {
    prop_oneof![
        Just(TapOutcome::Pass),
        Just(TapOutcome::Fail),
        Just(TapOutcome::Todo),
        Just(TapOutcome::Skip),
    ]
}

pub fn render_tap_case(outcomes: &[TapOutcome]) -> String {
    let mut input = String::from("TAP version 14\n");
    input.push_str(&format!("1..{}\n", outcomes.len()));

    for (index, outcome) in outcomes.iter().enumerate() {
        let id = index + 1;
        match outcome {
            TapOutcome::Pass => input.push_str(&format!("ok {id} - pass-{id}\n")),
            TapOutcome::Fail => input.push_str(&format!("not ok {id} - fail-{id}\n")),
            TapOutcome::Todo => {
                input.push_str(&format!("not ok {id} - todo-{id} # TODO waiting\n"))
            }
            TapOutcome::Skip => input.push_str(&format!("ok {id} - skip-{id} # SKIP ignored\n")),
        }
    }

    input
}

#[derive(Clone, Copy, Debug)]
pub enum JsonOutcome {
    Pass,
    Fail,
    Skip,
}

pub fn json_outcome_strategy() -> impl Strategy<Value = JsonOutcome> {
    prop_oneof![Just(JsonOutcome::Pass), Just(JsonOutcome::Fail), Just(JsonOutcome::Skip),]
}

pub fn render_go_json_case(outcomes: &[JsonOutcome]) -> String {
    let mut input = String::new();
    for (index, outcome) in outcomes.iter().enumerate() {
        let name = format!("Test{index}");
        let action = match outcome {
            JsonOutcome::Pass => "pass",
            JsonOutcome::Fail => "fail",
            JsonOutcome::Skip => "skip",
        };
        input.push_str(&format!("{{\"Action\":\"{action}\",\"Test\":\"{name}\"}}\n"));
    }
    input
}
