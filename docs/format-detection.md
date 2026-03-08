# Format Detection

`tapcue` supports three input format modes:

- `auto` (default)
- `tap`
- `json`

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
```

Or via config/environment:

- config: `[input] format = "auto|tap|json"`
- env: `TAPCUE_FORMAT=auto|tap|json`

To inspect detection decisions during runtime:

```bash
tapcue --trace-detection
```
