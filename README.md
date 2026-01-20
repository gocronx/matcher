# Matcher - High-Performance Trading Engine

[中文文档](README_CN.md)

---

High-performance trading matching engine built with Rust, supporting multiple order types and asynchronous processing.

## Features

### Core Functionality
- **High-Performance Matching Engine**: Sub-microsecond latency order matching
- **Multiple Order Types**: Market, Limit, IOC, FOK, Post-Only, Iceberg, GTD, Day
- **Async Architecture**: Tokio-based async runtime for high concurrency
- **Memory Optimization**: Object pool pre-allocation, SmallVec for reduced heap allocation, AHash for fast hashing
- **Comprehensive Monitoring**: Prometheus metrics integration, structured logging

### Technical Highlights
- **Lock-Free Algorithms**: Reduced contention, improved throughput
- **SIMD Optimization**: Leverages modern CPU features
- **Zero-Copy Networking**: Minimal memory allocations
- **Configuration-Driven**: TOML configuration file support
- **API-First Design**: REST and WebSocket interfaces

## Quick Start

### Installation

```bash
git clone https://github.com/gocronx/matcher.git
cd matcher
cargo build --release
```

### Basic Usage

```rust
use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = Config::default();
    
    // Create and start engine
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    
    let matching_engine = engine.matching_engine();
    
    // Submit limit order
    let order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50000,  // Price: $500.00
        100,    // Quantity: 100
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(order).await?;
    
    // View match results
    for m in matches {
        println!("Trade: {} @ {}", m.quantity, m.price);
    }
    
    Ok(())
}
```

### Configuration File

Create a `config.toml` file:

```toml
[engine]
product_ids = ["BTC-USD", "ETH-USD"]
max_orders_per_product = 1000000

[network]
listen_port = 8080
multicast_addr = "239.0.0.1:5000"
broadcast_addr = "239.0.0.2:5001"

[performance]
batch_size = 100
worker_threads = 4
use_fast_hash = true
use_object_pool = true
use_smallvec = true
```

## Order Types Explained

### 1. Market Order

Orders that execute immediately at the best available price.

```rust
let market_order = Order::market(
    "BTC-USD".to_string(),
    Side::Buy,
    100,  // Quantity
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(market_order).await?;
```

### 2. Limit Order

Orders with a specified price that only execute at or better than the specified price.

```rust
let limit_order = Order::limit(
    "BTC-USD".to_string(),
    Side::Sell,
    50100,  // Price: $501.00
    200,    // Quantity: 200
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(limit_order).await?;
```

### 3. Post-Only Order (Maker Only)

Orders that only add liquidity; rejected if they would immediately match.

```rust
let post_only = Order::post_only(
    "BTC-USD".to_string(),
    Side::Buy,
    49900,  // Price
    100,    // Quantity
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(post_only).await?;
// If order would immediately match, matches will be empty and order is rejected
```

### 4. Iceberg Order

Large orders that hide the true order size.

```rust
let iceberg = Order::iceberg(
    "BTC-USD".to_string(),
    Side::Sell,
    50100,  // Price
    1000,   // Total quantity
    100,    // Visible quantity
    current_timestamp_ns(),
);

let matches = matching_engine.submit_order(iceberg).await?;
// Order book shows only 100, but total is 1000
```

## API Reference

### Engine API

```rust
// Create engine
let mut engine = Engine::new(config).await?;

// Start engine
engine.start().await?;

// Get matching engine
let matching_engine = engine.matching_engine();
```

### MatchingEngine API

```rust
// Submit order
let matches = matching_engine.submit_order(order).await?;

// Cancel order
matching_engine.cancel_order(&order_id).await?;

// Get best bid and ask prices
let (best_bid, best_ask) = matching_engine.get_best_prices(&product_id)?;

// Get spread
let spread = matching_engine.get_spread(&product_id)?;

// Get order book snapshot
let (bids, asks) = matching_engine.get_order_book_snapshot(&product_id, 10)?;

// Get market depth
let (bid_depth, ask_depth) = matching_engine.get_depth(&product_id)?;

// Get engine statistics
let stats = matching_engine.get_stats();
```

### Order Construction Methods

```rust
// Market order
Order::market(product_id, side, quantity, timestamp)

// Limit order
Order::limit(product_id, side, price, quantity, timestamp)

// Post-Only order
Order::post_only(product_id, side, price, quantity, timestamp)

// Iceberg order
Order::iceberg(product_id, side, price, total_quantity, visible_quantity, timestamp)
```

## Running Examples

### Basic Matching Demo

```bash
cargo run --example basic_usage --release
```

Example output:
```
Starting Matcher Example
Engine started, submitting sample orders...
Submitted orders 1: buy_matches=0, sell_matches=0
...
Market order generated 2 matches:
  Match 1: 100 units at $501.00 (latency: 823ns)
  Match 2: 50 units at $502.00 (latency: 645ns)
```

### Advanced Features Demo

```bash
cargo run --example advanced_features --release
```

Example output:
```
Matcher v2.0 - Advanced Features Demo
Engine started with performance optimizations enabled
   - Fast Hash (ahash): ✓
   - Object Pool: ✓
   - SmallVec: ✓

Demo 1: Post-Only Order (Maker Only)
   Post-Only order submitted: 0 matches

Demo 2: Iceberg Order
   Iceberg order: total 1000, visible 100
   Matches: 0
```

### Complete Demo

```bash
cargo run --example complete_demo --release
```

## Performance Benchmarks

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench matching_benchmark
```

### Performance Metrics

| Metric | Matcher | Description |
|--------|---------|-------------|
| Latency (p50) | ~800ns | Median latency |
| Latency (p99) | ~2μs | 99th percentile latency |
| Throughput | 2M ops/sec | Operations per second |
| Memory Usage | ~50MB | For 1M orders |

## Testing

```bash
# Run all tests
cargo test --release

# Run unit tests
cargo test --lib

# Run integration tests
cargo test --test integration

# View test coverage
cargo tarpaulin --out Html
```

## Project Structure

```
matcher/
├── src/
│   ├── lib.rs           # Library entry point
│   ├── main.rs          # Executable entry point
│   ├── config/          # Configuration module
│   │   └── mod.rs       # Configuration definitions
│   ├── core/            # Core engine
│   │   ├── engine.rs    # Main engine
│   │   ├── matching.rs  # Matching engine
│   │   └── orderbook.rs # Order book
│   ├── network/         # Network module
│   │   └── mod.rs       # Network handling
│   ├── types/           # Type definitions
│   │   └── mod.rs       # Order, Match, etc.
│   └── utils/           # Utility module
│       ├── metrics.rs   # Metrics collection
│       ├── timer.rs     # High-resolution timer
│       └── mod.rs       # Utility functions
├── examples/            # Example code
│   ├── basic_usage.rs       # Basic usage
│   ├── advanced_features.rs # Advanced features
│   └── complete_demo.rs     # Complete demo
├── benches/             # Benchmarks
│   └── matching_benchmark.rs
├── tests/               # Integration tests
│   └── integration.rs
├── config.toml          # Configuration file
├── Cargo.toml           # Project configuration
└── README.md            # This file
```

## Monitoring and Metrics

Matcher provides comprehensive monitoring support:

### Prometheus Metrics

- `matcher_orders_received_total` - Total orders received
- `matcher_orders_matched_total` - Total orders matched
- `matcher_trades_executed_total` - Total trades executed
- `matcher_match_latency_seconds` - Match latency distribution
- `matcher_order_book_depth` - Order book depth
- `matcher_spread` - Bid-ask spread

### Structured Logging

```rust
use tracing::{info, warn, error};

info!("Order submitted: {:?}", order);
warn!("Order rejected: {:?}", reason);
error!("Engine error: {:?}", error);
```

## Dependencies

Main dependencies:
- `tokio`: Async runtime
- `serde`: Serialization framework
- `ahash`: Fast hash algorithm
- `smallvec`: Small vector optimization
- `slab`: Object pool
- `prometheus`: Metrics collection
- `tracing`: Structured logging

## Development Guide

### Build

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Check code
cargo check

# Format code
cargo fmt

# Code linting
cargo clippy
```

## FAQ

### Q: How to improve performance?

A: 
1. Compile with `--release` mode
2. Enable performance optimizations (fast_hash, object_pool, smallvec)
3. Adjust `batch_size` and `worker_threads`
4. Use SSD for WAL logs

### Q: Which order types are supported?

A: Currently supported:
- Market
- Limit
- IOC
- FOK
- Post-Only
- Iceberg
- GTD
- Day

### Q: How to integrate with existing systems?

A: Matcher provides multiple integration methods:
1. As a Rust library
2. Via REST API
3. Via WebSocket for real-time communication
4. Through message queues