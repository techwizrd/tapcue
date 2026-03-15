# tapcue

> Stream TAP or JSON test output and emit desktop notifications.
> More information: <https://github.com/techwizrd/tapcue>.

- Process TAP from `prove`:

`prove -v t/*.t | tapcue`

- Process `go test` JSON output:

`go test ./... -json | tapcue`

- Process `cargo nextest` JSON output:

`env NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format libtest-json-plus | tapcue`

- Force JSON mode and write summary JSON to stdout:

`vitest run --reporter=json | tapcue --format json --summary-format json --summary-file -`

- Validate merged config and exit:

`tapcue --validate-config`

- Print merged runtime config and exit:

`tapcue --print-effective-config`

- Check desktop notification readiness and disabled-notification reasons:

`tapcue doctor`

- Generate a local config file from built-in defaults:

`tapcue init`

- Generate a local config file from current effective settings:

`tapcue init --current`
