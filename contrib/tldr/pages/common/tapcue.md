# tapcue

> Stream TAP or JSON test output and emit desktop notifications.
> More information: <https://github.com/techwizrd/tapcue>.

- Process `npm test` TAP output:

`npm test --silent | tapcue`

- Process default Bun test output:

`bun test | tapcue`

- Process `go test` JSON output:

`go test ./... -json | tapcue`

- Process `cargo nextest` JSON output:

`env NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run --message-format libtest-json-plus | tapcue`

- CI mode: disable desktop notifications and write summary JSON:

`go test ./... -json | tapcue --no-notify --summary-format json --summary-file run-summary.json`

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
