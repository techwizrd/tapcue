## What

Describe the change in 1-3 bullets.

## Why

Explain the motivation and user impact.

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo check --all-targets --all-features --locked`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-features --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`

## Checklist

- [ ] Tests added/updated as needed
- [ ] Documentation updated as needed
- [ ] Changelog updated for user-visible changes
