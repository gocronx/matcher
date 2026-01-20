use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with colors
    tracing_subscriber::fmt()
        .with_env_filter("matcher=info")
        .with_target(false)
        .init();
    
    info!("╔════════════════════════════════════════════════════════════╗");
    info!("║  Matcher - Comprehensive Demo                             ║");
    info!("║  High-Performance Trading Engine                          ║");
    info!("╚════════════════════════════════════════════════════════════╝");
    
    // Step 1: Create and configure engine
    info!("\nStep 1: Creating Engine Configuration");
    let mut config = Config::default();
    config.performance.use_fast_hash = true;
    config.performance.use_object_pool = true;
    config.performance.use_smallvec = true;
    
    info!("Fast Hash (ahash): enabled");
    info!("Object Pool: enabled");
    info!("SmallVec: enabled");
    
    // Step 2: Initialize and start engine
    info!("\nStep 2: Initializing Engine");
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    let matching_engine = engine.matching_engine();
    info!("Engine started successfully");
    
    // Step 3: Build order book with limit orders
    info!("\nStep 3: Building Order Book");
    info!("Placing sell orders (asks)...");
    
    for i in 1..=5 {
        let sell_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50000 + (i * 100),  // Prices: 50100, 50200, ..., 50500
            10 * i,             // Quantities: 10, 20, ..., 50
            current_timestamp_ns(),
        );
        
        let matches = matching_engine.submit_order(sell_order).await?;
        info!("Sell order #{}: price={}, qty={}, matches={}", 
            i, 50000 + (i * 100), 10 * i, matches.len());
    }
    
    info!("Placing buy orders (bids)...");
    for i in 1..=5 {
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            49900 - (i * 100),  // Prices: 49800, 49700, ..., 49400
            10 * i,             // Quantities: 10, 20, ..., 50
            current_timestamp_ns(),
        );
        
        let matches = matching_engine.submit_order(buy_order).await?;
        info!("Buy order #{}: price={}, qty={}, matches={}", 
            i, 49900 - (i * 100), 10 * i, matches.len());
    }
    
    // Step 4: Display order book state
    info!("\nStep 4: Order Book State");
    let (bids, asks) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 5)?;
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    let spread = matching_engine.get_spread(&"BTC-USD".to_string())?;
    
    info!("Best Bid: {:?}", best_bid);
    info!("Best Ask: {:?}", best_ask);
    if let Some(s) = spread {
        info!("Spread: {} (${:.2})", s, s as f64 / 100.0);
    }
    
    info!("\nTop 5 Bids:");
    for (i, level) in bids.iter().enumerate() {
        info!("{}. ${:.2} × {}", i + 1, level.price as f64 / 100.0, level.quantity);
    }
    
    info!("\nTop 5 Asks:");
    for (i, level) in asks.iter().enumerate() {
        info!("{}. ${:.2} × {}", i + 1, level.price as f64 / 100.0, level.quantity);
    }
    
    // Step 5: Market order execution
    info!("\nStep 5: Market Order Execution");
    let market_buy = Order::market(
        "BTC-USD".to_string(),
        Side::Buy,
        75,  // Buy 75 units at market price
        current_timestamp_ns(),
    );
    
    info!("Submitting market buy order for 75 units...");
    let matches = matching_engine.submit_order(market_buy).await?;
    
    info!("Market order executed with {} matches:", matches.len());
    let mut total_qty = 0;
    let mut total_value = 0;
    
    for (i, m) in matches.iter().enumerate() {
        info!("Match {}: {} units @ ${:.2} (latency: {}ns)", 
              i + 1, m.quantity, m.price as f64 / 100.0, m.match_latency_ns);
        total_qty += m.quantity;
        total_value += m.quantity * m.price;
    }
    
    if total_qty > 0 {
        let avg_price = total_value / total_qty;
        info!("Average fill price: ${:.2}", avg_price as f64 / 100.0);
    }
    
    // Step 6: Post-Only order demo
    info!("\nStep 6: Post-Only Order (Maker Only)");
    let post_only = Order::post_only(
        "BTC-USD".to_string(),
        Side::Buy,
        50100,  // This price would cross the spread
        50,
        current_timestamp_ns(),
    );
    
    info!("Submitting Post-Only order at ${:.2}...", 50100 as f64 / 100.0);
    let matches = matching_engine.submit_order(post_only).await?;
    
    if matches.is_empty() {
        warn!("Post-Only order rejected (would take liquidity)");
    } else {
        info!("Post-Only order accepted: {} matches", matches.len());
    }
    
    // Step 7: Iceberg order demo
    info!("\nStep 7: Iceberg Order");
    let iceberg = Order::iceberg(
        "BTC-USD".to_string(),
        Side::Sell,
        50600,  // Above current market
        500,    // Total quantity
        50,     // Visible quantity
        current_timestamp_ns(),
    );
    
    info!("Submitting Iceberg order:");
    info!("Total quantity: 500");
    info!("Visible quantity: 50");
    info!("Price: ${:.2}", 50600 as f64 / 100.0);
    
    let matches = matching_engine.submit_order(iceberg).await?;
    info!("Iceberg order placed: {} matches", matches.len());
    
    // Step 8: Multiple small orders to demonstrate throughput
    info!("\nStep 8: Throughput Test");
    let start = std::time::Instant::now();
    let num_orders = 1000;
    
    info!("Submitting {} orders...", num_orders);
    
    for i in 0..num_orders {
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        let price = if side == Side::Buy {
            49000 - (i % 10) * 100
        } else {
            51000 + (i % 10) * 100
        };
        
        let order = Order::limit(
            "BTC-USD".to_string(),
            side,
            price,
            10,
            current_timestamp_ns(),
        );
        
        matching_engine.submit_order(order).await?;
    }
    
    let elapsed = start.elapsed();
    let throughput = num_orders as f64 / elapsed.as_secs_f64();
    
    info!("Processed {} orders in {:.2}ms", num_orders, elapsed.as_millis());
    info!("Throughput: {:.0} orders/sec", throughput);
    
    // Step 9: Final statistics
    info!("\nStep 9: Engine Statistics");
    let stats = matching_engine.get_stats();
    
    info!("Orders Received: {}", stats.orders_received);
    info!("Orders Matched: {}", stats.orders_matched);
    info!("Trades Executed: {}", stats.trades_executed);
    info!("Average Match Latency: {}ns ({:.2}μs)", 
          stats.avg_match_latency_ns,
          stats.avg_match_latency_ns as f64 / 1000.0);
    info!("Uptime: {}s", stats.uptime_seconds);

    // Step 10: Final order book snapshot
    info!("\nStep 10: Final Order Book Snapshot");
    let (bid_depth, ask_depth) = matching_engine.get_depth(&"BTC-USD".to_string())?;
    let (final_bids, final_asks) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 3)?;
    
    info!("Total Depth: {} bids, {} asks", bid_depth, ask_depth);
    info!("\nTop 3 Bids:");
    for (i, level) in final_bids.iter().enumerate() {
        info!("{}. ${:.2} × {}", i + 1, level.price as f64 / 100.0, level.quantity);
    }
    
    info!("\nTop 3 Asks:");
    for (i, level) in final_asks.iter().enumerate() {
        info!("{}. ${:.2} × {}", i + 1, level.price as f64 / 100.0, level.quantity);
    }
    
    // Keep engine running briefly
    info!("\nKeeping engine running for 5 seconds...");
    sleep(Duration::from_secs(5)).await;
    
    info!("\n╔════════════════════════════════════════════════════════════╗");
    info!("║  Demo Completed Successfully!                              ║");
    info!("║  Tip: Run with --release for best performance             ║");
    info!("╚════════════════════════════════════════════════════════════╝\n");
    
    Ok(())
}
