# tapcue

> Stream TAP or JSON test output and emit desktop notifications.
> More information: <https://github.com/techwizrd/tapcue>.

- Run `npm test` TAP output through `tapcue`:

`tapcue run -- npm test --silent`

- Run default Bun test output through `tapcue`:

`tapcue run -- bun test`

- Run `go test` JSON output through `tapcue`:

`tapcue run -- go test ./...`

- Run `cargo nextest` JSON output through `tapcue`:

`tapcue run -- cargo nextest run`

- Run `pytest` with auto-inferred JUnit output:

`tapcue run -- pytest`

- CI mode: disable desktop notifications and write summary JSON:

`tapcue --no-notify --summary-format json --summary-file run-summary.json run -- go test ./...`

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
