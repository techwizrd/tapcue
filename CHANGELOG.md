# Changelog

All notable changes to this project will be documented in this file.

The format follows Keep a Changelog and the project adheres to Semantic Versioning.

## [Unreleased]

### Added
- TAP and JSON stream processing for common runners (go test, nextest, Jest, Vitest).
- Recursive TAP subtest parsing with strict-mode protocol enforcement and TAP13 compatibility.
- Runner integration checks for Rust, Go, npm TAP, Jest JSON, Vitest JSON, pytest TAP, and unittest TAP.
- Man page source (`docs/man/tapcue.1`) and TLDR page source (`docs/tldr/tapcue.md`).
- `tapcue doctor` diagnostics command for desktop notification readiness, config source resolution, and disabled-notification explanations.
- `tapcue init` command to generate `./.tapcue.toml` from defaults or current effective config.
- Property-based parser tests for TAP/JSON stream robustness.
- Bun test output support (including default Bun text and dot-style progress lines) with auto-detection and `--format bun`.
- Native Bun runner integration check (`scripts/verify-bun-integration.sh`) and CI job coverage.

### Changed
- Line-buffer hot path to indexed extraction/compaction for improved tiny-chunk stream performance.
- Hooking workflow standardized on `prek` with root `.pre-commit-config.yaml` compatibility.
- Desktop diagnostics command is now `tapcue doctor` (subcommand form).
- macOS notifications now prefer `terminal-notifier` when available, with `osascript` fallback.
- macOS notification copy now matches Linux wording for failure, bailout, and run-summary subtitles while preserving structured summary body text.
- CI now publishes a single rolling prerelease tag (`unreleased`) on untagged `main` pushes.
- CI now runs Rust checks on Linux/macOS/Windows and adds Linux toolchain coverage for 1.86.0, stable, and nightly (nightly allowed to fail).
- Pull request validation checklist now matches CONTRIBUTING required checks (`cargo check --locked` and `cargo doc --locked`).

### Fixed
- CR-only TAP line endings now parse correctly (`\r` separators).
