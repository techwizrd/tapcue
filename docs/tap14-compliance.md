# TAP14 Compliance

This document summarizes TAP14 behavior implemented by `tapcue`.

## Compatibility baseline

- Accepts `TAP version 14`.
- Accepts `TAP version 13` for compatibility.
- Requires a plan (`1..N`) exactly once.

## Plan handling

- Missing plan is a protocol failure.
- Plan mismatch (`planned != observed`) fails the run.
- `1..0` with no test points is treated as a successful skip-all run.

## Test point handling

- Supports `ok` and `not ok` with optional ID and description.
- Implicit numbering is supported when IDs are omitted.
- `TODO` and `SKIP` directives are case-insensitive and matched by exact directive token.

## Invalid lines and strict mode

- By default, invalid/non-TAP lines are warnings and do not fail the run.
- `pragma +strict` enables strict mode; invalid lines become protocol failures.
- `pragma -strict` disables strict mode.
- Unexpected indentation outside recognized subtest and YAML diagnostics is treated as invalid.

## Bailouts

- `Bail out!` is treated case-insensitively.
- A bailout fails the run and preserves the bailout reason.

## Subtests

- 4-space-indented subtest bodies are parsed recursively as nested TAP streams.
- Nested subtest parse/protocol failures propagate to the parent run.
- A completed subtest must be followed by its parent test point.
- Parent correlated test point must not contradict nested subtest success/failure.

## YAML diagnostics

- YAML diagnostics are accepted only when they immediately follow a test point.
- YAML diagnostics must start with `  ---` and end with `  ...`.

## Line endings

- `\r\n` and `\r` line endings are accepted.

## Test coverage

Behavior above is validated by:

- `tests/tap14_conformance.rs`
- unit tests in `src/processor.rs`
