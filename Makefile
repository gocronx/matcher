# Matcher - High-Performance Trading Engine Makefile

.PHONY: help build test bench clean run example config fmt clippy doc

# Default target
help:
	@echo "Matcher - High-Performance Trading Engine"
	@echo ""
	@echo "Available targets:"
	@echo "  build     - Build the project in release mode"
	@echo "  test      - Run all tests"
	@echo "  bench     - Run performance benchmarks"
	@echo "  run       - Run the matching engine with default config"
	@echo "  example   - Run the basic usage example"
	@echo "  config    - Generate default configuration file"
	@echo "  fmt       - Format code with rustfmt"
	@echo "  clippy    - Run clippy linter"
	@echo "  doc       - Generate documentation"
	@echo "  clean     - Clean build artifacts"

# Build targets
build:
	@echo "🔨 Building matcher in release mode..."
	cargo build --release

debug:
	@echo "🔨 Building matcher in debug mode..."
	cargo build

# Test targets
test:
	@echo "🧪 Running tests..."
	cargo test

test-verbose:
	@echo "🧪 Running tests with verbose output..."
	cargo test -- --nocapture

# Benchmark targets
bench:
	@echo "⚡ Running performance benchmarks..."
	cargo bench

bench-baseline:
	@echo "⚡ Running benchmarks and saving baseline..."
	cargo bench -- --save-baseline main

# Run targets
run: build
	@echo "🚀 Starting matcher with default configuration..."
	./target/release/matcher

run-debug:
	@echo "🚀 Starting matcher in debug mode..."
	cargo run

example:
	@echo "📖 Running basic usage example..."
	cargo run --example basic_usage

# Configuration
config:
	@echo "⚙️  Generating default configuration..."
	cargo run -- --generate-config

config-custom:
	@echo "⚙️  Generating configuration with custom settings..."
	./target/release/matcher --generate-config --config custom-config.toml

# Code quality
fmt:
	@echo "🎨 Formatting code..."
	cargo fmt

clippy:
	@echo "📎 Running clippy linter..."
	cargo clippy -- -D warnings

check:
	@echo "✅ Checking code..."
	cargo check

# Documentation
doc:
	@echo "📚 Generating documentation..."
	cargo doc --open

doc-private:
	@echo "📚 Generating documentation (including private items)..."
	cargo doc --document-private-items --open

# Maintenance
clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean

update:
	@echo "📦 Updating dependencies..."
	cargo update

# Performance profiling
profile: build
	@echo "📊 Running with profiling..."
	perf record -g ./target/release/matcher
	perf report

# Memory analysis
valgrind: build
	@echo "🔍 Running memory analysis..."
	valgrind --tool=memcheck --leak-check=full ./target/release/matcher

# Docker targets (if needed)
docker-build:
	@echo "🐳 Building Docker image..."
	docker build -t matcher:latest .

docker-run:
	@echo "🐳 Running in Docker..."
	docker run -p 8080:8080 -p 9090:9090 matcher:latest

# Development helpers
dev-setup:
	@echo "🛠️  Setting up development environment..."
	rustup component add rustfmt clippy
	cargo install cargo-watch cargo-audit

watch:
	@echo "👀 Watching for changes..."
	cargo watch -x check -x test

audit:
	@echo "🔒 Auditing dependencies for security vulnerabilities..."
	cargo audit

# Release preparation
pre-release: fmt clippy test bench
	@echo "🎯 Pre-release checks completed successfully!"

release-patch:
	@echo "🏷️  Creating patch release..."
	cargo release patch

release-minor:
	@echo "🏷️  Creating minor release..."
	cargo release minor

release-major:
	@echo "🏷️  Creating major release..."
	cargo release major