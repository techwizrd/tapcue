use tapcue::processor::RunState;

pub(crate) fn merge_run_state(state: &mut RunState, add: &RunState) {
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

pub(crate) fn empty_run_state() -> RunState {
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
