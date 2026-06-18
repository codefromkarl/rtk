# Godot Engine

> Part of [`src/cmds/`](../README.md) — see also [docs/TECHNICAL.md](../../../docs/TECHNICAL.md)

## Specifics

- `godot_cmd.rs` filters Godot engine CLI output for export, check, test, script, and import flows
- `export` / `import` collapse noisy import logs into compact summaries
- `check` groups script parse errors by file and line
- `test` detects both GUT and GdUnit4 output and shows failures only
- `script` strips engine startup noise while preserving script-produced output
- `run_other()` is passthrough for unsupported Godot subcommands
