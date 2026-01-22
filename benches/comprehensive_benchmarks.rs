use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use matcher::*;
use matcher::core::order_book::OrderBook;
use matcher::core::MatchingEngine;
use matcher::utils::current_timestamp_ns;
use tokio::runtime::Runtime;
use std::sync::Arc;

// Benchmark order book operations
fn bench_order_book_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("OrderBook Operations");
    
    // Benchmark adding orders
    group.bench_function("add_limit_order", |b| {
        let book = OrderBook::new("BTC-USD".to_string());
        let mut counter = 0u64;
        
        b.iter(|| {
            counter += 1;
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                50000 + (counter % 1000), // Vary price slightly
                100,
                current_timestamp_ns() + counter,
            );
            black_box(book.add_order(order).unwrap());
        });
    });
    
    // Benchmark order matching
    group.bench_function("match_order", |b| {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Pre-populate with resting orders
        for i in 0..1000 {
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Sell,
                51000 + i,
                100,
                current_timestamp_ns() + i,
            );
            book.add_order(order).unwrap();
        }
        
        let mut counter = 0u64;
        b.iter(|| {
            counter += 1;
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                51000 + (counter % 500), // Will match some orders
                50,
                current_timestamp_ns() + counter + 10000,
            );
            black_box(book.match_order(order, current_timestamp_ns()));
        });
    });
    
    // Benchmark market data queries
    group.bench_function("get_best_prices", |b| {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add some orders
        for i in 0..100 {
            let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000 - i, 100, i);
            let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i + 1000);
            book.add_order(buy_order).unwrap();
            book.add_order(sell_order).unwrap();
        }
        
        b.iter(|| {
            black_box((book.best_bid(), book.best_ask()));
        });
    });
    
    group.bench_function("get_order_book_snapshot", |b| {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add many orders
        for i in 0..1000 {
            let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000 - i, 100, i);
            let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i + 10000);
            book.add_order(buy_order).unwrap();
            book.add_order(sell_order).unwrap();
        }
        
        b.iter(|| {
            black_box(book.snapshot(10));
        });
    });
    
    group.finish();
}

// Benchmark different order types
fn bench_order_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("Order Types");
    
    for order_type in ["Market", "Limit", "PostOnly", "Iceberg"].iter() {
        group.bench_with_input(BenchmarkId::new("match_order", order_type), order_type, |b, &order_type| {
            let book = OrderBook::new("BTC-USD".to_string());
            
            // Add resting orders
            for i in 0..100 {
                let order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i);
                book.add_order(order).unwrap();
            }
            
            let mut counter = 0u64;
            b.iter(|| {
                counter += 1;
                let order = match order_type {
                    "Market" => Order::market("BTC-USD".to_string(), Side::Buy, 50, counter),
                    "Limit" => Order::limit("BTC-USD".to_string(), Side::Buy, 51050, 50, counter),
                    "PostOnly" => Order::post_only("BTC-USD".to_string(), Side::Buy, 50000, 50, counter),
                    "Iceberg" => Order::iceberg("BTC-USD".to_string(), Side::Buy, 51050, 200, 50, counter),
                    _ => unreachable!(),
                };
                black_box(book.match_order(order, current_timestamp_ns()));
            });
        });
    }
    
    group.finish();
}

// Benchmark MatchingEngine operations
fn bench_matching_engine(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("MatchingEngine");
    
    group.bench_function("submit_order", |b| {
        let mut config = Config::default();
        config.engine.wal_path = None; // Disable WAL for benchmarking
        let engine = rt.block_on(MatchingEngine::new(config)).unwrap();
        let engine = Arc::new(engine);
        
        let mut counter = 0u64;
        b.iter(|| {
            let engine = engine.clone();
            counter += 1;
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                50000 + (counter % 1000),
                100,
                current_timestamp_ns() + counter,
            );
            rt.block_on(async {
                black_box(engine.submit_order(order).await.unwrap());
            });
        });
    });
    
    group.bench_function("cancel_order", |b| {
        let mut config = Config::default();
        config.engine.wal_path = None; // Disable WAL for benchmarking
        let engine = rt.block_on(MatchingEngine::new(config)).unwrap();
        let engine = Arc::new(engine);
        
        // Pre-populate with orders to cancel
        let mut order_ids = Vec::new();
        for i in 0..100 {
            let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000 + i, 100, i);
            let order_id = order.id;
            rt.block_on(engine.submit_order(order)).unwrap();
            order_ids.push(order_id);
        }
        
        let mut index = 0;
        b.iter(|| {
            let engine = engine.clone();
            let order_id = order_ids[index % order_ids.len()];
            index += 1;
            
            rt.block_on(async {
                // Note: This will fail after first cancellation, but we're measuring the attempt
                let _ = engine.cancel_order(&"BTC-USD".to_string(), order_id).await;
            });
        });
    });
    
    group.finish();
}

// Benchmark concurrent operations
fn bench_concurrent_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("Concurrent Operations");
    
    for thread_count in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("concurrent_submit", thread_count),
            thread_count,
            |b, &thread_count| {
                let mut config = Config::default();
                config.engine.wal_path = None; // Disable WAL for benchmarking
                let engine = rt.block_on(MatchingEngine::new(config)).unwrap();
                let engine = Arc::new(engine);
                
                b.iter(|| {
                    let engine = engine.clone();
                    rt.block_on(async move {
                        let mut handles = Vec::new();
                        
                        for i in 0..thread_count {
                            let engine = engine.clone();
                            let handle = tokio::spawn(async move {
                                let order = Order::limit(
                                    "BTC-USD".to_string(),
                                    if i % 2 == 0 { Side::Buy } else { Side::Sell },
                                    50000 + (i * 100),
                                    100,
                                    current_timestamp_ns() + i as u64,
                                );
                                engine.submit_order(order).await.unwrap()
                            });
                            handles.push(handle);
                        }
                        
                        for handle in handles {
                            black_box(handle.await.unwrap());
                        }
                    });
                });
            },
        );
    }
    
    group.finish();
}

// Benchmark memory usage patterns
fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("Memory Patterns");
    
    // Benchmark with different order book sizes
    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("large_order_book", size),
            size,
            |b, &size| {
                b.iter(|| {
                    let book = OrderBook::new("BTC-USD".to_string());
                    
                    // Fill order book
                    for i in 0..size {
                        let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000 - i, 100, i);
                        let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i + size);
                        book.add_order(buy_order).unwrap();
                        book.add_order(sell_order).unwrap();
                    }
                    
                    // Perform some operations
                    black_box(book.best_bid());
                    black_box(book.best_ask());
                    black_box(book.snapshot(10));
                });
            },
        );
    }
    
    group.finish();
}

// Latency measurement benchmark
fn bench_latency_measurement(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("Latency Measurement");
    
    // Configure for latency measurement
    group.measurement_time(std::time::Duration::from_secs(10));
    group.sample_size(1000);
    
    group.bench_function("end_to_end_latency", |b| {
        let mut config = Config::default();
        config.engine.wal_path = None; // Disable WAL for benchmarking
        let engine = rt.block_on(MatchingEngine::new(config)).unwrap();
        let engine = Arc::new(engine);
        
        // Pre-populate with resting orders
        rt.block_on(async {
            for i in 0..100 {
                let order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i);
                engine.submit_order(order).await.unwrap();
            }
        });
        
        let mut counter = 0u64;
        b.iter(|| {
            let engine = engine.clone();
            counter += 1;
            rt.block_on(async move {
                let order = Order::limit(
                    "BTC-USD".to_string(),
                    Side::Buy,
                    51000 + (counter % 50), // Will match some orders
                    50,
                    current_timestamp_ns() + counter,
                );
                
                let start = std::time::Instant::now();
                let matches = engine.submit_order(order).await.unwrap();
                let duration = start.elapsed();
                
                black_box((matches, duration));
            });
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_order_book_operations,
    bench_order_types,
    bench_matching_engine,
    bench_concurrent_operations,
    bench_memory_patterns,
    bench_latency_measurement
);

criterion_main!(benches);