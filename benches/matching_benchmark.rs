use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;
use tokio::runtime::Runtime;

fn bench_order_matching(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // Setup engine
    let config = Config::default();
    let engine = rt.block_on(async {
        Engine::new(config).await.unwrap()
    });
    
    let matching_engine = engine.matching_engine();
    
    // Pre-populate order book with some orders
    rt.block_on(async {
        for i in 0..1000 {
            let buy_order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                50000 - i, // Decreasing prices
                100,
                current_timestamp_ns(),
            );
            
            let sell_order = Order::limit(
                "BTC-USD".to_string(),
                Side::Sell,
                50000 + i, // Increasing prices
                100,
                current_timestamp_ns(),
            );
            
            let _ = matching_engine.submit_order(buy_order).await;
            let _ = matching_engine.submit_order(sell_order).await;
        }
    });
    
    let mut group = c.benchmark_group("order_matching");
    
    // Benchmark limit order submission (no match)
    group.bench_function("limit_order_no_match", |b| {
        b.iter(|| {
            rt.block_on(async {
                let order = Order::limit(
                    "BTC-USD".to_string(),
                    Side::Buy,
                    black_box(40000), // Price that won't match
                    black_box(100),
                    current_timestamp_ns(),
                );
                
                let _ = matching_engine.submit_order(order).await;
            })
        });
    });
    
    // Benchmark market order (will match)
    group.bench_function("market_order_match", |b| {
        b.iter(|| {
            rt.block_on(async {
                let order = Order::market(
                    "BTC-USD".to_string(),
                    Side::Buy,
                    black_box(50),
                    current_timestamp_ns(),
                );
                
                let _ = matching_engine.submit_order(order).await;
            })
        });
    });
    
    // Benchmark different order sizes
    for size in [1, 10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("market_order_by_size", size),
            size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async {
                        let order = Order::market(
                            "BTC-USD".to_string(),
                            Side::Buy,
                            black_box(size),
                            current_timestamp_ns(),
                        );
                        
                        let _ = matching_engine.submit_order(order).await;
                    })
                });
            },
        );
    }
    
    group.finish();
}

fn bench_order_book_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let config = Config::default();
    let engine = rt.block_on(async {
        Engine::new(config).await.unwrap()
    });
    
    let matching_engine = engine.matching_engine();
    
    let mut group = c.benchmark_group("order_book_operations");
    
    // Benchmark order book snapshot
    group.bench_function("order_book_snapshot", |b| {
        b.iter(|| {
            let _ = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), black_box(10));
        });
    });
    
    // Benchmark best price retrieval
    group.bench_function("best_prices", |b| {
        b.iter(|| {
            let _ = matching_engine.get_best_prices(&"BTC-USD".to_string());
        });
    });
    
    // Benchmark spread calculation
    group.bench_function("spread_calculation", |b| {
        b.iter(|| {
            let _ = matching_engine.get_spread(&"BTC-USD".to_string());
        });
    });
    
    group.finish();
}

fn bench_high_res_timer(c: &mut Criterion) {
    use matcher::utils::HighResTimer;
    
    let mut group = c.benchmark_group("timing");
    
    // Benchmark timer creation
    group.bench_function("timer_creation", |b| {
        b.iter(|| {
            let _timer = HighResTimer::start();
        });
    });
    
    // Benchmark timer measurement
    group.bench_function("timer_measurement", |b| {
        let timer = HighResTimer::start();
        b.iter(|| {
            black_box(timer.elapsed_ns());
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_order_matching,
    bench_order_book_operations,
    bench_high_res_timer
);
criterion_main!(benches);