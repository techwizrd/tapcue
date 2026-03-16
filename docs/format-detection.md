# Format Detection

`tapcue` supports three input format modes:

- `auto` (default)
- `tap`
- `json`
- `bun`

JUnit XML is ingested from explicit report paths/globs via CLI flags
(`--junit-file`, `--junit-dir`, `--junit-glob`) rather than stdin auto-detection.
In `tapcue run -- ...` mode, JUnit auto-discovery defaults are applied for
common tools (`gradle`/`gradlew`, `mvn`/`mvnw`) unless disabled with
`--no-auto-junit-reports`.

## How `auto` works

`auto` inspects incoming stream content line-by-line and picks a parser once it has a strong signal.

JSON signals:

- first non-empty line starts with `{` or `[`

TAP signals:

- `TAP version ...`
- `ok ...`
- `not ok ...`
- `Bail out! ...`
- plan line such as `1..42`

Bun signals:

- `bun test ...`
- `(pass) ...` / `(fail) ...`
- dot-progress lines such as `..F.S`
- `failures:` section header

If no explicit signal appears before EOF, `tapcue` falls back to:

- JSON if trimmed input begins with `{` or `[`
- otherwise TAP

## Permissive JSON behavior

When JSON mode is selected (explicitly or by auto-detection), parsing is permissive:

- non-JSON noise lines are skipped
- malformed JSON lines do not abort the run
- processing continues for subsequent valid JSON entries

This is intended to handle mixed tool output in real CI/dev environments.

## Override detection

You can force parser selection:

```bash
tapcue --format tap
tapcue --format json
tapcue --format bun
```

Or via config/environment:

- config: `[input] format = "auto|tap|json|bun"`
- env: `TAPCUE_FORMAT=auto|tap|json|bun`

To inspect detection decisions during runtime:

```bash
tapcue --trace-detection
```

## Notes for Bun

Some Bun versions/environments emit reporter lines on stderr. In plain pipe mode,
merge stderr into stdout:

```bash
bun test 2>&1 | tapcue
```

Or use command mode:

```bash
tapcue run -- bun test
```
