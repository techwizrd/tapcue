# tapcue

[![CI](https://img.shields.io/github/actions/workflow/status/techwizrd/tapcue/ci.yml?branch=main)](https://github.com/techwizrd/tapcue/actions/workflows/ci.yml)
[![GitHub Release](https://img.shields.io/github/v/release/techwizrd/tapcue?sort=semver)](https://github.com/techwizrd/tapcue/releases)
[![MSRV](https://img.shields.io/badge/MSRV-1.75-blue?logo=rust)](https://github.com/techwizrd/tapcue/blob/main/Cargo.toml)
[![License](https://img.shields.io/github/license/techwizrd/tapcue)](LICENSE)

`tapcue` reads TAP (Test Anything Protocol) and JSON test output from `stdin` and sends desktop notifications for:

- failing tests
- bailouts
- final run summary

It is designed for streaming TAP input and incremental parsing. 🦀

Common runner inputs include:

- `go test -json`
- `cargo nextest --message-format libtest-json-plus`
- `jest --json`
- `vitest --reporter=json`
- `bun test`

## Runner examples

These commands are local/dev-style runs that trigger desktop notifications:

```bash
go test ./... -json | tapcue
env NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format libtest-json-plus | tapcue
npm test --silent | tapcue
bun test 2>&1 | tapcue
jest --json --outputFile /dev/stdout | tapcue
vitest run --reporter=json | tapcue
pytest --tap-stream | tapcue
```

Or let `tapcue` run a command and capture both stdout+stderr itself:

```bash
tapcue run -- bun test
tapcue run -- go test ./... -json
```

## Install / Build

```bash
cargo build --release
```

The resulting binary is at `target/release/tapcue`.

Install directly from GitHub `main`:

```bash
cargo install --git https://github.com/techwizrd/tapcue --branch main tapcue
```

Optional pin/reinstall variants:

```bash
cargo install --git https://github.com/techwizrd/tapcue --rev <commit-sha> tapcue
cargo install --git https://github.com/techwizrd/tapcue --branch main tapcue --force
```

## CLI options and subcommands

- `--quiet-parse-errors`: suppress parse warnings for malformed TAP
- `--no-quiet-parse-errors`: force parse warnings on
- `--strict`: enforce TAP14 strict mode even if test output has `pragma -strict`
- `--no-notify`: disable desktop notifications (useful in CI/tests)
- `--notify`: force desktop notifications on
- `--desktop <auto|force-on|force-off>`: override desktop notification detection
- `--format <auto|tap|json|bun>`: input parsing format (default: `auto`)
- `--summary-format <none|text|json>`: emit run summary for automation
- `--summary-file <path|->`: write summary output to a file or stdout (`-`)
- `--dedup-failures` / `--no-dedup-failures`: control repeated failure notifications
- `--max-failure-notifications <N>`: cap emitted failure notifications per run
- `--trace-detection`: print auto format detection decisions
- `--validate-config`: validate merged config and exit
- `--print-effective-config`: print merged config and exit
- `doctor`: check desktop notification readiness and explain why notifications are disabled
- `init [--current] [--force]`: write `./.tapcue.toml` from defaults or current effective config

## Configuration

`tapcue` reads TOML configuration from two locations:

- user config: platform standard config directory (`.../tapcue/config.toml`)
- optional project override: `./.tapcue.toml`

Precedence is:

`CLI flags > environment variables > local config > user config > defaults`

Boolean CLI flags are explicit overrides. For example, `--notify` can force
notifications on even if config files or environment disabled them.

Supported environment variables:

- `TAPCUE_QUIET_PARSE_ERRORS` (`true/false`)
- `TAPCUE_NO_NOTIFY` (`true/false`)
- `TAPCUE_NOTIFICATIONS_ENABLED` (`true/false`)
- `TAPCUE_DESKTOP` (`auto`, `force-on`, `force-off`)
- `TAPCUE_FORMAT` (`auto`, `tap`, `json`, `bun`)
- `TAPCUE_SUMMARY_FORMAT` (`none`, `text`, `json`)
- `TAPCUE_SUMMARY_FILE` (path)
- `TAPCUE_DEDUP_FAILURES` (`true/false`)
- `TAPCUE_MAX_FAILURE_NOTIFICATIONS` (integer)
- `TAPCUE_TRACE_DETECTION` (`true/false`)

macOS notification backend notes:

- `tapcue` uses `terminal-notifier` automatically when it is installed and available in `PATH`.
- Otherwise it falls back to `osascript` (available on standard macOS installs).

Notification backend requirements by platform:

- Linux: `notify-send` available in `PATH` (usually from `libnotify-bin`).
- macOS: `terminal-notifier` in `PATH`, or built-in `osascript`.
- Windows: `powershell` in `PATH`.

Example `.tapcue.toml`:

```toml
[parser]
quiet_parse_errors = false
strict = false

[input]
format = "auto"

[notifications]
enabled = true
desktop = "auto"
dedup_failures = true
max_failure_notifications = 20

[output]
summary_format = "none"
# summary_file = "tapcue-summary.json"
```

Generate a local config file:

```bash
tapcue init

# include user/local/env merged values
tapcue init --current

# overwrite existing .tapcue.toml
tapcue init --force
```

Dogfooding with this repository's tests:

```bash
./scripts/dogfood-nextest.sh
```

Automation-friendly summary example:

```bash
go test ./... -json | tapcue --summary-format json --summary-file run-summary.json
```

CI-oriented mode (no desktop notifications, emit machine-readable summary):

```bash
go test ./... -json | tapcue --no-notify --summary-format json --summary-file run-summary.json
```

Emit summary JSON to stdout explicitly:

```bash
go test ./... -json | tapcue --summary-format json --summary-file -
```

Detailed auto-detection and parser behavior is documented in
`docs/format-detection.md`.

To inspect the final merged settings at runtime:

```bash
tapcue --print-effective-config

tapcue doctor
```

For complete CLI documentation, run:

```bash
tapcue --help
```

Manual pages and quick command examples:

- Man page source: `docs/man/tapcue.1`
- TLDR/tealdeer page source: `docs/tldr/tapcue.md`
- TLDR upstream submission copy: `contrib/tldr/pages/common/tapcue.md`

## Development

Install optional dev tools (including `cargo-nextest`) with:

```bash
./scripts/bootstrap-dev-tools.sh
```

Manual install for nextest:

```bash
cargo install --locked cargo-nextest
```

```bash
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
cargo bench --features benchmarks --bench stream_processing
```

Primary hook runner:

```bash
prek run --all-files
prek run --hook-stage manual --all-files
```

Cross-runner integration verification (Rust nextest, Go, npm TAP, Bun default output, Jest JSON, Vitest JSON, pytest TAP, unittest TAP):

```bash
./scripts/verify-runner-integrations.sh
```

Individual integration checks:

```bash
./scripts/verify-nextest-integration.sh
./scripts/verify-go-integration.sh
./scripts/verify-npm-tap-integration.sh
./scripts/verify-bun-integration.sh
./scripts/verify-jest-integration.sh
./scripts/verify-vitest-integration.sh
./scripts/verify-pytest-integration.sh
./scripts/verify-unittest-integration.sh
```

## Project docs

- Contributing guide: `CONTRIBUTING.md`
- Security policy: `SECURITY.md`
- Support information: `SUPPORT.md`
- Changelog: `CHANGELOG.md`
- TAP14 compliance notes: `docs/tap14-compliance.md`
- Man page source: `docs/man/tapcue.1`
- TLDR/tealdeer source: `docs/tldr/tapcue.md`
- TLDR upstream submission copy: `contrib/tldr/pages/common/tapcue.md`
