# Changelog

All notable changes to this project will be documented in this file.

The format follows Keep a Changelog and the project adheres to Semantic Versioning.

## [Unreleased]

### Added
- Initial release of `tapcue`.
- Streaming TAP parsing with incremental processing using `tap_parser`.
- Desktop notifications for failures, bailouts, and completion summary.
- Layered configuration with precedence:
  `CLI flags > environment > local config > user config > defaults`.
- CI pipeline with formatting, clippy, tests, docs, release packaging.
- Pre-commit checks via `prek`.
