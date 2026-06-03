---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Security

> This file extends [common/security.md](../common/security.md) with Rust specific content.

## Secret Management

```rust
// Read from environment, fail fast if missing
let db_url = std::env::var("DATABASE_URL")
    .expect("DATABASE_URL must be set");
```

## `unsafe` Policy

- Never use `unsafe` without a documented justification comment
- Each `unsafe` block must explain why it is sound
- Prefer safe abstractions (e.g. `bytemuck`, `zerocopy`) over raw `unsafe`

## Dependency Auditing

```bash
cargo audit          # Check for known vulnerabilities (cargo-audit)
cargo deny check     # License and advisory checks (cargo-deny)
```

## SQL Injection

**The rule is parameterization, not a specific library.** Never format or
concatenate user input into a SQL string; always pass it as a bound parameter.
This is what prevents injection — an ORM is not required for safety (and an ORM's
raw/literal escape hatch can still inject if misused).

```rust
// BAD — string interpolation, injectable regardless of library
let q = format!("SELECT * FROM users WHERE id = {user_input}");

// GOOD — bound parameter ($1 / ?1 / ? depending on driver)
```

Any of these satisfy the rule:

- **SQLx** (recommended default) — parameterized **and** compile-time-checked
  against the real schema: `sqlx::query!("SELECT * FROM users WHERE id = $1", id)`.
- **rusqlite** — `conn.execute("... WHERE id = ?1", params![id])`.
- **Diesel / SeaORM** — type-safe query builders; safe as long as you stay on the
  builder API and avoid raw-SQL escape hatches.

Choose by need: SQL-first control → SQLx/rusqlite; heavy CRUD or dynamic queries →
Diesel/SeaORM. Safety is equal — the difference is boilerplate vs. control.

## Input Validation

Validate at the boundary — use types to enforce invariants so invalid data can't reach business logic.
