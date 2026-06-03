---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Coding Style

> Extends [common/coding-style.md](../common/coding-style.md). Apply on every `.rs` edit.

## Formatting

`rustfmt` is mandatory — run on every save.
`clippy` warnings are errors in CI: `cargo clippy --workspace --all-targets -- -D warnings`

Enforce the function-length budget as a clippy gate, not just prose:

```toml
# clippy.toml (workspace root)
too-many-lines-threshold = 50
too-many-arguments-threshold = 5   # clippy default is 7; see "Too Many Parameters" below
```

```rust
// each crate root (lib.rs / main.rs)
#![warn(clippy::too_many_lines)]
```

With `-D warnings` in CI, the warn becomes a hard failure. Clippy counts
logical lines (comments and blanks excluded), so it doesn't punish documentation.

## `#[allow]` Policy

**`allow` is for "the lint misfired on a sound design" — never for "the lint is
right but I don't want to fix it."** Every `#[allow]` must carry a reason
comment; a bare `#[allow]` is a review blocker.

| Lint | Allow? | Why |
|------|--------|-----|
| `too_many_lines` | Sometimes | Long-but-flat sequential logic (a command-dispatch `match`) reads worse when split |
| `too_many_arguments` | Almost never | It's a modeling signal: aggregate into a struct, or the `Option` cluster into an enum |
| `module_name_repetitions` | Often | Naming-taste lint, frequently misfires |
| `unwrap_used` (in tests) | Yes | Tests should panic loudly |

```rust
// GOOD: visible, justified exception
#[allow(clippy::too_many_lines)] // flat dispatch match; splitting hurts readability
async fn main() { ... }

// BAD: silencing a design signal instead of fixing the design
#[allow(clippy::too_many_arguments)]
pub async fn run_fork(a: ..., b: ..., /* 7 more */) { ... }
```

Never raise a threshold in `clippy.toml` to make one offender pass — that
dilutes the gate for every function. Exempt the one case with a justified
`#[allow]` instead.

## Too Many Parameters — Aggregate Into Types

More than ~5 parameters means a concept is missing its type. Mutually-exclusive
`Option` parameters are the strongest signal — model them as an enum so illegal
combinations stop compiling:

```rust
// BAD: 3 Options whose legality is enforced by a runtime match
async fn run_fork(set_response: Option<String>, from_run: Option<String>,
                  from_at: Option<i64>, /* ... 6 more */) -> Result<()>

// GOOD: the concept gets a type; (None, None) can no longer exist
enum OverrideSource {
    File(String),
    FromRun { run: String, at: Option<i64> },
}
struct ForkArgs { parent_ref: String, at: i64, source: OverrideSource, port: u16 }
async fn run_fork(store: Arc<Store>, args: ForkArgs) -> Result<()>
```

Same move as enum-with-data for modal state (see `rust-patterns` →
"Make Invalid States Unrepresentable").

## Naming Conventions

| Construct | Convention |
|-----------|------------|
| Variables, functions, modules, files | `snake_case` |
| Types, traits, enums | `PascalCase` |
| Constants, statics | `SCREAMING_SNAKE_CASE` |
| Lifetimes | `'a`, `'src` (short, lowercase) |
| Type parameters | `T`, `K`, `V`, or descriptive `Item` |

## Ownership — Three Rules

```rust
// Borrow (&T) when you only read
fn len(s: &str) -> usize { s.len() }

// Borrow mutably (&mut T) when you modify in place
fn push(v: &mut Vec<u8>, b: u8) { v.push(b) }

// Own (T) only when you store or transfer
fn into_upper(s: String) -> String { s.to_uppercase() }
```

**Never `.clone()` to silence the borrow checker** — fix the design instead.
Prefer `&str` over `String` for function parameters.

## Error Handling

`?` everywhere — never `.unwrap()` or `.expect()` in production:

```rust
// BAD
let data = fs::read_to_string(path).unwrap();

// GOOD: ? propagates, .with_context() adds meaning
let data = fs::read_to_string(path)
    .with_context(|| format!("reading {path}"))?;
```

- `anyhow` for applications (binary crates)
- `thiserror` for libraries (library crates)

## `match` Over `if let` Chains

```rust
// BAD: hard to follow, silently misses cases
if let Some(user) = get_user(id) {
    if let Some(email) = user.email {
        send(email);
    }
}

// GOOD: exhaustive, readable
match get_user(id).and_then(|u| u.email) {
    Some(email) => send(email),
    None => tracing::warn!("no email for {id}"),
}
```

## Visibility — Least Privilege

```rust
// Private by default (no keyword) — always prefer
struct InternalCache { ... }

// pub(crate) — visible within this crate only
pub(crate) fn helper() { ... }

// pub(super) — visible to parent module only
pub(super) struct SubImpl { ... }

// pub — only for intentional public API
pub struct Client { ... }
```

Never `pub` unless it is part of the intended public API.

## `#[must_use]`

Attach to functions whose return value must not be silently ignored:

```rust
#[must_use]
fn build(self) -> Config { ... }  // compiler warns if caller drops the return
```

## Default Derives

Always derive these on data types unless there's a reason not to:

```rust
#[derive(Debug, Clone, PartialEq)]
struct User { id: u64, name: String }

// Add Eq + Hash when used as map keys
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UserId(u64);
```

## Module Organization

```
src/
├── lib.rs          # crate root — re-exports public API only
├── error.rs        # AppError + From impls
├── config.rs       # Config struct
└── user/
    ├── mod.rs      # pub use; no logic here
    ├── model.rs    # User struct
    └── repo.rs     # UserRepo trait + impl
```

One concern per module. `mod.rs` re-exports, does not implement.

## Reference

See skill `rust-patterns` for ownership, async/Tokio, Axum, SQLx, tracing, and testing.
