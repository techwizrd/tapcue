# Contributing

Thanks for your interest in contributing to `tapcue`.

## Development setup

```bash
cargo build
./scripts/bootstrap-dev-tools.sh
```

If you only want nextest:

```bash
cargo install --locked cargo-nextest
```

Dogfood nextest integration locally:

```bash
./scripts/dogfood-nextest.sh
```

## Required checks

Before opening a PR, run:

```bash
cargo fmt --all --check
cargo check --all-targets --all-features --locked
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
```

Optional benchmark run:

```bash
cargo bench --features benchmarks --bench stream_processing
```

Cross-runner integration verification (requires Go, Node.js/npm, Python, uv, cargo-nextest):

```bash
./scripts/verify-runner-integrations.sh
```

Or run individual checks:

```bash
./scripts/verify-nextest-integration.sh
./scripts/verify-go-integration.sh
./scripts/verify-npm-tap-integration.sh
./scripts/verify-jest-integration.sh
./scripts/verify-vitest-integration.sh
./scripts/verify-pytest-integration.sh
./scripts/verify-unittest-integration.sh
```

You can also run repository hooks with:

```bash
prek run --all-files
prek run --hook-stage manual --all-files
```

If you update Python fixture dependencies, regenerate lockfiles with hashes:

```bash
uv pip compile --generate-hashes -o tests/runner-fixtures/python-pytest/requirements.lock tests/runner-fixtures/python-pytest/requirements.txt
uv pip compile --generate-hashes -o tests/runner-fixtures/python-unittest/requirements.lock tests/runner-fixtures/python-unittest/requirements.txt
```

## Pull requests

- Keep changes focused and small when possible.
- Include tests for behavioral changes.
- Update docs (`README.md`, config examples, changelog) when relevant.
- Use clear commit messages explaining intent.

## Release notes

Add user-visible changes to `CHANGELOG.md` under `[Unreleased]`.
