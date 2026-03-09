# Changelog

All notable changes to this project will be documented in this file.

The format follows Keep a Changelog and the project adheres to Semantic Versioning.

## [Unreleased]

### Added
- TAP and JSON stream processing for common runners (go test, nextest, Jest, Vitest).
- Recursive TAP subtest parsing with strict-mode protocol enforcement and TAP13 compatibility.
- Runner integration checks for Rust, Go, npm TAP, Jest JSON, Vitest JSON, pytest TAP, and unittest TAP.
- Man page source (`docs/man/tapcue.1`) and TLDR page source (`docs/tldr/tapcue.md`).

### Changed
- Line-buffer hot path to indexed extraction/compaction for improved tiny-chunk stream performance.
- Hooking workflow standardized on `prek` with root `.pre-commit-config.yaml` compatibility.

### Fixed
- CR-only TAP line endings now parse correctly (`\r` separators).
