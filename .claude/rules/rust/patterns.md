---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Patterns

> This file extends [common/patterns.md](../common/patterns.md) with Rust specific content.

## Type-Driven Design

Make invalid states unrepresentable — encode constraints in the type system, not runtime checks.

```rust
// Newtype for zero-cost semantic distinction
struct UserId(u64);
struct OrderId(u64);
// Can't mix them up at compile time
```

## Builder with Typestate

Use `PhantomData` to encode required steps at compile time — missing a step is a compile error, not a runtime panic.

## Error Handling

```rust
// ? propagates, .context() adds meaning
fn load(path: &str) -> Result<Config> {
    let s = fs::read_to_string(path)
        .with_context(|| format!("reading {path}"))?;
    toml::from_str(&s).context("parsing config")
}
```

## Iterators over Loops

```rust
// Prefer iterator chains — zero overhead, parallelizable with rayon
items.iter()
    .filter(|i| i.active)
    .map(|i| i.score * 2)
    .collect::<Vec<_>>()
```

## Reference

See skill: `rust-patterns` for comprehensive Rust patterns including async/Tokio, Axum, SQLx, and full-stack examples.
