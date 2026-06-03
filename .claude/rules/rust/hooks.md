---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Hooks

## PostToolUse Hook (Claude Code `settings.json`)

Auto-format (`rustfmt`) and clippy on every `.rs` edit. The easiest way is
cct's scaffold, which installs a ready-made hook script and wires it up:

```bash
./scaffold.sh <project-dir> rust
# installs .claude/hooks/post-edit-lint.cjs + .claude/settings.json
```

Manual setup — hook input arrives as JSON on stdin (there is no
`$CLAUDE_TOOL_INPUT_FILE_PATH` env var), so use the script:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Edit|Write|MultiEdit",
        "hooks": [
          {
            "type": "command",
            "command": "node \"$CLAUDE_PROJECT_DIR/.claude/hooks/post-edit-lint.cjs\""
          }
        ]
      }
    ]
  }
}
```

- `rustfmt` — formats the edited file immediately
- `cargo clippy` — runs on the edited file's crate, findings reported as warnings
- `cargo check` intentionally omitted — too slow for per-save; run manually or in CI

## Pre-commit (`.pre-commit-config.yaml`)

```yaml
repos:
  - repo: local
    hooks:
      - id: rustfmt
        name: rustfmt
        entry: cargo fmt --all --
        language: system
        types: [rust]
        pass_filenames: false

      - id: clippy
        name: clippy
        entry: cargo clippy -- -D warnings
        language: system
        types: [rust]
        pass_filenames: false
```

## CI Quality Gate

```bash
cargo fmt --all -- --check        # formatting check
cargo clippy -- -D warnings       # lint as errors
cargo audit                       # vulnerability scan
cargo deny check                  # license + advisory check
cargo test --all-features         # full test suite
```
