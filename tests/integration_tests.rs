use matcher::{Config, Engine, Order, Side};
use matcher::utils::current_timestamp_ns;

#[tokio::test]
async fn test_engine_initialization() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let engine = Engine::new(config).await.expect("Failed to create engine");
    
    let matching_engine = engine.matching_engine();
    let stats = matching_engine.get_stats();
    
    assert_eq!(stats.orders_received, 0);
    assert_eq!(stats.orders_matched, 0);
    assert_eq!(stats.trades_executed, 0);
}

#[tokio::test]
async fn test_order_submission_and_matching() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Submit a limit sell order
    let sell_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Sell,
        50000, // $500.00
        100,   // 100 units
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(sell_order).await
        .expect("Failed to submit sell order");
    
    assert_eq!(matches.len(), 0, "Sell order should not match initially");
    
    // Submit a matching buy order
    let buy_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50000, // Same price
        50,    // Partial quantity
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(buy_order).await
        .expect("Failed to submit buy order");
    
    assert_eq!(matches.len(), 1, "Buy order should generate one match");
    
    let match_result = &matches[0];
    assert_eq!(match_result.price, 50000);
    assert_eq!(match_result.quantity, 50);
    assert_eq!(match_result.product_id, "BTC-USD");
}

#[tokio::test]
async fn test_market_order_matching() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Add some liquidity to the book
    for i in 0..5 {
        let sell_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            50000 + (i * 100), // Prices from $500.00 to $504.00
            100,
            current_timestamp_ns(),
        );
        
        matching_engine.submit_order(sell_order).await
            .expect("Failed to submit sell order");
    }

    // Submit market buy order
    let market_order = Order::market(
        "BTC-USD".to_string(),
        Side::Buy,
        250, // Should match against multiple levels
        current_timestamp_ns(),
    );
    
    let matches = matching_engine.submit_order(market_order).await
        .expect("Failed to submit market order");
    
    assert!(matches.len() >= 2, "Market order should match multiple levels");
    
    let total_matched: u64 = matches.iter().map(|m| m.quantity).sum();
    assert_eq!(total_matched, 250, "All quantity should be matched");
}

#[tokio::test]
async fn test_order_cancellation() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Submit an order
    let order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50000,
        100,
        current_timestamp_ns(),
    );
    
    let order_id = order.id;
    matching_engine.submit_order(order).await
        .expect("Failed to submit order");
    
    // Cancel the order
    let cancelled_order = matching_engine.cancel_order(&"BTC-USD".to_string(), order_id).await
        .expect("Failed to cancel order");
    
    assert_eq!(cancelled_order.id, order_id);
    
    // Try to cancel again (should fail)
    let result = matching_engine.cancel_order(&"BTC-USD".to_string(), order_id).await;
    assert!(result.is_err(), "Cancelling non-existent order should fail");
}

#[tokio::test]
async fn test_order_book_operations() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Initially empty book
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())
        .expect("Failed to get best prices");
    assert_eq!(best_bid, None);
    assert_eq!(best_ask, None);
    
    // Add some orders
    let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 49900, 100, current_timestamp_ns());
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 50100, 100, current_timestamp_ns());
    
    matching_engine.submit_order(buy_order).await.expect("Failed to submit buy order");
    matching_engine.submit_order(sell_order).await.expect("Failed to submit sell order");
    
    // Check best prices
    let (best_bid, best_ask) = matching_engine.get_best_prices(&"BTC-USD".to_string())
        .expect("Failed to get best prices");
    assert_eq!(best_bid, Some(49900));
    assert_eq!(best_ask, Some(50100));
    
    // Check spread
    let spread = matching_engine.get_spread(&"BTC-USD".to_string())
        .expect("Failed to get spread");
    assert_eq!(spread, Some(200)); // $2.00 spread
    
    // Check depth
    let (bid_depth, ask_depth) = matching_engine.get_depth(&"BTC-USD".to_string())
        .expect("Failed to get depth");
    assert_eq!(bid_depth, 1);
    assert_eq!(ask_depth, 1);
}

#[tokio::test]
async fn test_invalid_orders() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Test unsupported product
    let invalid_order = Order::limit(
        "INVALID-PRODUCT".to_string(),
        Side::Buy,
        50000,
        100,
        current_timestamp_ns(),
    );
    
    let result = matching_engine.submit_order(invalid_order).await;
    assert!(result.is_err(), "Invalid product should be rejected");
    
    // Test zero quantity
    let zero_qty_order = Order::limit(
        "BTC-USD".to_string(),
        Side::Buy,
        50000,
        0, // Zero quantity
        current_timestamp_ns(),
    );
    
    let result = matching_engine.submit_order(zero_qty_order).await;
    assert!(result.is_err(), "Zero quantity should be rejected");
}

#[tokio::test]
async fn test_performance_under_load() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let mut engine = Engine::new(config).await.expect("Failed to create engine");
    engine.start().await.expect("Failed to start engine");
    
    let matching_engine = engine.matching_engine();
    
    // Submit many orders quickly
    let start_time = std::time::Instant::now();
    let num_orders = 1000;
    
    for i in 0..num_orders {
        let order = Order::limit(
            "BTC-USD".to_string(),
            if i % 2 == 0 { Side::Buy } else { Side::Sell },
            50000 + ((i % 100) * 100) as u64, // Varying prices in multiples of tick size
            100,
            current_timestamp_ns(),
        );
        
        matching_engine.submit_order(order).await
            .expect("Failed to submit order");
    }
    
    let elapsed = start_time.elapsed();
    let orders_per_second = num_orders as f64 / elapsed.as_secs_f64();
    
    println!("Processed {} orders in {:?} ({:.0} orders/sec)", 
             num_orders, elapsed, orders_per_second);
    
    // Should be able to process at least 100 orders per second (relaxed for debug builds)
    assert!(orders_per_second > 100.0, "Performance too low: {} orders/sec", orders_per_second);
}

#[tokio::test]
async fn test_wal_persistence() {
    // Generate a unique path for the WAL file
    let wal_path = format!("target/test_wal_{}", 
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    
    // 1. Start engine and place orders
    {
        let mut config = Config::default();
        config.engine.wal_path = Some(wal_path.clone());
        let mut engine = Engine::new(config).await.expect("Failed to create engine 1");
        engine.start().await.expect("Start 1");
        
        let matching_engine = engine.matching_engine();
        
        let order = Order::limit(
            "BTC-USD".to_string(), 
            Side::Buy, 
            50000, 
            100, 
            match matcher::utils::current_timestamp_ns() { 0 => 1, x => x } // ensure non-zero
        );
        matching_engine.submit_order(order).await.expect("Submit 1");
        
        // Ensure data is flushed (submit_order flushes)
    } // engine dropped here
    
    // 2. Restart engine with same WAL
    {
        let mut config = Config::default();
        config.engine.wal_path = Some(wal_path.clone());
        let engine = Engine::new(config).await.expect("Failed to create engine 2");
        // Replay happens in new()
        
        let matching_engine = engine.matching_engine();
        // Check depth
        let (bid_depth, ask_depth) = matching_engine.get_depth(&"BTC-USD".to_string()).unwrap();
        assert_eq!(bid_depth, 1, "Should have restored 1 bid order");
        assert_eq!(ask_depth, 0);
        
        // Verify order content
        let (bids, _) = matching_engine.get_order_book_snapshot(&"BTC-USD".to_string(), 1).unwrap();
        assert_eq!(bids[0].price, 50000);
        assert_eq!(bids[0].quantity, 100);
    }
    
    // Cleanup
    let _ = std::fs::remove_file(wal_path);
}