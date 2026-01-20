use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;
use std::time::Duration;
use tokio::time::sleep;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let _ = tracing_subscriber::fmt::try_init();
    
    info!("🚀 Starting Matcher Example");
    
    // Create configuration
    let config = Config::default();
    
    // Create and start engine
    let mut engine = Engine::new(config).await?;
    engine.start().await?;
    
    let matching_engine = engine.matching_engine();
    
    info!("Engine started, submitting sample orders...");
    
    // Submit some limit orders to build the order book
    for i in 0..10 {
        // Buy orders (bids)
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            50000 - (i * 100), // Prices from $500.00 down to $491.00
            100 + (i * 10),    // Quantities from 100 to 190
            current_timestamp_ns(),
        );
        
        // Sell orders (asks)
        let sell_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50100 + (i * 100), // Prices from $501.00 up to $510.00
            100 + (i * 10),    // Quantities from 100 to 190
            current_timestamp_ns(),
        );
        
        let buy_matches = matching_engine.submit_order(buy_order).await?;
        let sell_matches = matching_engine.submit_order(sell_order).await?;
        
        info!("Submitted orders {}: buy_matches={}, sell_matches={}", 
              i + 1, buy_matches.len(), sell_matches.len());
    }
    
    // Show order book state
    let (bids, asks) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 5)?;
    info!("Order book snapshot:");
    info!("Top 5 Bids: {:?}", bids);
    info!("Top 5 Asks: {:?}", asks);
    
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())?;
    info!("Best bid: {:?}, Best ask: {:?}", best_bid, best_ask);
    
    if let Some(spread) = matching_engine.get_spread(&"BTC-USD".to_string())? {
        info!("Spread: {} cents", spread);
    }
    
    // Submit a market buy order that will match
    info!("Submitting market buy order...");
    let market_order = Order::market(
        "BTC-USD".to_string(),
        Side::Buy,
        150, // Buy 150 units at market price
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(market_order).await?;
    info!("Market order generated {} matches:", matches.len());
    
    for (i, match_result) in matches.iter().enumerate() {
        info!("  Match {}: {} units at ${:.2} (latency: {}ns)", 
              i + 1,
              match_result.quantity,
              match_result.price as f64 / 100.0,
              match_result.match_latency_ns);
    }
    
    // Show updated order book
    let (bids, asks) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 3)?;
    info!("Updated order book (top 3 levels):");
    info!("Bids: {:?}", bids);
    info!("Asks: {:?}", asks);
    
    // Show engine statistics
    let stats = matching_engine.get_stats();
    info!("Engine statistics:");
    info!("  Orders received: {}", stats.orders_received);
    info!("  Orders matched: {}", stats.orders_matched);
    info!("  Trades executed: {}", stats.trades_executed);
    info!("  Average match latency: {}ns", stats.avg_match_latency_ns);
    info!("  Uptime: {}s", stats.uptime_seconds);
    
    // Keep running for a bit to show metrics
    info!("Example completed. Engine will continue running for 10 seconds...");
    sleep(Duration::from_secs(10)).await;
    
    info!("Example finished!");
    
    Ok(())
}