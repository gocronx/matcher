---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
  - "**/Cargo.lock"
---
# Rust Testing

> This file extends [common/testing.md](../common/testing.md) with Rust specific content.

## Test Organization

- Unit tests live in the same file as the code under `#[cfg(test)] mod tests`
- Integration tests live in `tests/` at the crate root
- Use `#[tokio::test]` for async tests

## Running Tests

```bash
cargo test                  # All tests
cargo test --test integration  # Integration tests only
cargo test -- --nocapture   # Show println! output
cargo test -p crate-name    # Specific crate in workspace
```

## Key Patterns

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_error_on_empty_input() {
        assert!(parse("").is_err());
    }

    #[tokio::test]
    async fn fetch_returns_404_for_unknown_id() {
        let db = test_db().await;
        assert!(matches!(
            get_user(&db, Uuid::new_v4()).await,
            Err(AppError::NotFound(_))
        ));
    }
}
```

## Property Testing

Use `proptest` for invariant testing — especially for parsers and data transformations:

```rust
proptest! {
    #[test]
    fn parse_never_panics(s in ".*") {
        let _ = parse(&s); // must not panic on any input
    }
}
```

## Reference

See skill: `rust-patterns` for full testing examples including async tests and property-based tests.
