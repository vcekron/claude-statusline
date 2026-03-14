# claude-statusline

A custom status line provider for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). Shows workspace info, model, context usage, API rate limit utilization, and extra usage costs — all in one line.

This is a personal tool. It's not flexible, not user-friendly, and configured via compile-time constants. If you want to use it, you'll need to read the source and adjust it yourself.

## What it shows

```
workspace │ Opus 4.6 │ 0% │ 31% ███░░┃░░░░ 05:00 │ 3% ┃░░░░░░░░░ Sat 00:00 │ €17.89
```

- **Directory** — current working directory name
- **Model** — active Claude model
- **Context** — context window usage %
- **5h utilization** — rate limit usage with progress bar, pace marker, and reset time
- **7d utilization** — weekly rate limit usage
- **Extra usage** — extra credits spent (if enabled)

Each segment can be toggled on/off via `const` flags in `main.rs`.

## Requirements

- macOS (uses `security` CLI for keychain access and libc `localtime_r`)
- Rust 2024 edition
- An active Claude Code OAuth session (credentials stored in macOS Keychain)

## Build

```sh
cargo build --release
```

## Setup

Point your Claude Code status line config at the binary:

```jsonc
// ~/.claude/settings.json
{
  "statusLine": {
    "command": "/path/to/claude-statusline"
  }
}
```

## License

MIT
