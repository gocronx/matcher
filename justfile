# Justfile - Task runner configuration

# Run all tests
test:
    cargo test --all-features

# Run benchmarks
bench:
    cargo bench

# Generate coverage report (HTML format)
coverage:
    cargo tarpaulin --out Html --output-dir ./coverage

# Run fuzzing tests (requires cargo-fuzz)
fuzz-codec:
    cargo +nightly fuzz run fuzz_codec -- -max_total_time=60

fuzz-snapshot:
    cargo +nightly fuzz run fuzz_snapshot -- -max_total_time=60

fuzz-book:
    cargo +nightly fuzz run fuzz_order_book -- -max_total_time=300

# Run all fuzz tests
fuzz-all: fuzz-codec fuzz-snapshot fuzz-book

# Check code quality
check:
    cargo clippy -- -D warnings
    cargo fmt -- --check

# Full CI pipeline
ci: check test coverage
