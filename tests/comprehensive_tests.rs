use matcher::*;
use matcher::types::{MatcherError, TimeInForce, OrderMetadata, Side, OrderType};
use matcher::core::order_book::OrderBook;
use matcher::core::MatchingEngine;
use matcher::storage::wal::{WalManager, LogEntry};
use matcher::utils::current_timestamp_ns;
use tempfile::TempDir;

// ============================================================================
// Basic Functionality Tests
// ============================================================================

#[tokio::test]
async fn test_order_submission_and_matching() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for simplicity
    let engine = MatchingEngine::new(config).await.unwrap();
    
    // Test 1: Submit order without match
    let order1 = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let matches1 = engine.submit_order(order1).await.unwrap();
    assert_eq!(matches1.len(), 0);
    
    // Test 2: Submit matching order
    let order2 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 50, current_timestamp_ns() + 1000);
    let matches2 = engine.submit_order(order2).await.unwrap();
    assert_eq!(matches2.len(), 1);
    assert_eq!(matches2[0].price, 50000);
    assert_eq!(matches2[0].quantity, 50);
}

#[tokio::test]
async fn test_order_cancellation() {
    let mut config = Config::default();
    config.engine.wal_path = None;
    let engine = MatchingEngine::new(config).await.unwrap();
    
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let order_id = order.id;
    let product_id = order.product_id.clone();
    
    engine.submit_order(order).await.unwrap();
    
    // Cancel the order
    let cancelled = engine.cancel_order(&product_id, order_id).await.unwrap();
    assert_eq!(cancelled.id, order_id);
    
    // Try to cancel again - should fail
    let result = engine.cancel_order(&product_id, order_id).await;
    assert!(matches!(result, Err(MatcherError::OrderNotFound { .. })));
}

#[tokio::test]
async fn test_market_data_queries() {
    let mut config = Config::default();
    config.engine.wal_path = None;
    let engine = MatchingEngine::new(config).await.unwrap();
    
    // Initially empty
    let (best_bid, best_ask) = engine.get_best_prices(&"BTC-USD".to_string()).unwrap();
    assert_eq!(best_bid, None);
    assert_eq!(best_ask, None);
    
    // Add orders
    let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 49000, 100, current_timestamp_ns());
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000, 100, current_timestamp_ns() + 1000);
    
    engine.submit_order(buy_order).await.unwrap();
    engine.submit_order(sell_order).await.unwrap();
    
    // Check best prices
    let (best_bid, best_ask) = engine.get_best_prices(&"BTC-USD".to_string()).unwrap();
    assert_eq!(best_bid, Some(49000));
    assert_eq!(best_ask, Some(51000));
    
    // Check spread
    let spread = engine.get_spread(&"BTC-USD".to_string()).unwrap();
    assert_eq!(spread, Some(2000));
    
    // Check depth
    let (bid_depth, ask_depth) = engine.get_depth(&"BTC-USD".to_string()).unwrap();
    assert_eq!(bid_depth, 1);
    assert_eq!(ask_depth, 1);
    
    // Check snapshot
    let (bids, asks) = engine.get_order_book_snapshot(&"BTC-USD".to_string(), 10).unwrap();
    assert_eq!(bids.len(), 1);
    assert_eq!(asks.len(), 1);
    assert_eq!(bids[0].price, 49000);
    assert_eq!(asks[0].price, 51000);
}

// ============================================================================
// Order Book Tests
// ============================================================================

#[test]
fn test_order_book_operations() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Test empty book
    assert_eq!(book.best_bid(), None);
    assert_eq!(book.best_ask(), None);
    assert_eq!(book.spread(), None);
    
    // Add buy order
    let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    book.add_order(buy_order).unwrap();
    
    assert_eq!(book.best_bid(), Some(50000));
    assert_eq!(book.best_ask(), None);
    
    // Add sell order
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000, 100, current_timestamp_ns() + 1000);
    book.add_order(sell_order).unwrap();
    
    assert_eq!(book.best_bid(), Some(50000));
    assert_eq!(book.best_ask(), Some(51000));
    assert_eq!(book.spread(), Some(1000));
}

#[test]
fn test_post_only_orders() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add a sell order
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, current_timestamp_ns());
    book.add_order(sell_order).unwrap();
    
    // Try Post-Only buy order at same price (should be rejected)
    let post_only = Order::post_only("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns() + 1000);
    let matches = book.match_order(post_only, current_timestamp_ns());
    assert_eq!(matches.len(), 0);
    
    // Post-Only at lower price should work
    let post_only_lower = Order::post_only("BTC-USD".to_string(), Side::Buy, 49000, 100, current_timestamp_ns() + 2000);
    let matches_lower = book.match_order(post_only_lower, current_timestamp_ns());
    assert_eq!(matches_lower.len(), 0); // No match, but order should be added
}

#[test]
fn test_iceberg_orders() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add iceberg order: total 1000, visible 100
    let iceberg = Order::iceberg("BTC-USD".to_string(), Side::Sell, 50000, 1000, 100, current_timestamp_ns());
    book.add_order(iceberg).unwrap();
    
    // Check snapshot shows only visible quantity
    let (_, asks) = book.snapshot(10);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].quantity, 100); // Only visible quantity
    assert_eq!(asks[0].price, 50000);
}

#[test]
fn test_market_orders() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add some resting orders
    let sell1 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 50, current_timestamp_ns());
    let sell2 = Order::limit("BTC-USD".to_string(), Side::Sell, 50100, 50, current_timestamp_ns() + 1000);
    book.add_order(sell1).unwrap();
    book.add_order(sell2).unwrap();
    
    // Market buy order should match at best prices
    let market_buy = Order::market("BTC-USD".to_string(), Side::Buy, 75, current_timestamp_ns() + 2000);
    let matches = book.match_order(market_buy, current_timestamp_ns());
    
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].price, 50000); // Better price first
    assert_eq!(matches[0].quantity, 50);
    assert_eq!(matches[1].price, 50100);
    assert_eq!(matches[1].quantity, 25);
}

// ============================================================================
// WAL Tests
// ============================================================================

#[tokio::test]
async fn test_wal_basic_operations() {
    let temp_dir = TempDir::new().unwrap();
    let wal_path = temp_dir.path().join("test.wal");
    
    // Create WAL and write some entries
    let wal = WalManager::new(&wal_path).unwrap();
    
    let order1 = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let order2 = Order::limit("BTC-USD".to_string(), Side::Sell, 51000, 50, current_timestamp_ns() + 1000);
    
    wal.append(&LogEntry::PlaceOrder(order1.clone())).unwrap();
    wal.append(&LogEntry::PlaceOrder(order2.clone())).unwrap();
    wal.append(&LogEntry::CancelOrder(order1.id, "BTC-USD".to_string())).unwrap();
    
    // Read back entries
    let entries = wal.replay().unwrap();
    assert_eq!(entries.len(), 3);
    
    match &entries[0] {
        LogEntry::PlaceOrder(order) => assert_eq!(order.id, order1.id),
        _ => panic!("Expected PlaceOrder"),
    }
    
    match &entries[2] {
        LogEntry::CancelOrder(order_id, product_id) => {
            assert_eq!(*order_id, order1.id);
            assert_eq!(product_id, "BTC-USD");
        }
        _ => panic!("Expected CancelOrder"),
    }
}

#[tokio::test]
async fn test_wal_integration_with_engine() {
    let temp_dir = TempDir::new().unwrap();
    let wal_path = temp_dir.path().join("engine_test.wal");
    
    // Create engine with WAL
    let mut config = Config::default();
    config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
    
    let engine = MatchingEngine::new(config.clone()).await.unwrap();
    
    // Submit some orders
    let order1 = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let order2 = Order::limit("BTC-USD".to_string(), Side::Sell, 51000, 50, current_timestamp_ns() + 1000);
    
    engine.submit_order(order1).await.unwrap();
    engine.submit_order(order2).await.unwrap();
    
    let stats1 = engine.get_stats();
    
    // Create new engine with same WAL (simulates restart)
    let engine2 = MatchingEngine::new(config).await.unwrap();
    let stats2 = engine2.get_stats();
    
    // Stats should be restored
    assert_eq!(stats1.orders_received, stats2.orders_received);
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_config_validation() {
    // Valid config should pass
    let config = Config::default();
    assert!(config.validate().is_ok());
    
    // Invalid configs should fail
    let mut invalid_config = Config::default();
    
    // Empty product IDs
    invalid_config.engine.product_ids.clear();
    assert!(invalid_config.validate().is_err());
    
    // Reset and test zero max orders
    invalid_config = Config::default();
    invalid_config.engine.max_orders_per_product = 0;
    assert!(invalid_config.validate().is_err());
    
    // Reset and test zero port
    invalid_config = Config::default();
    invalid_config.network.listen_port = 0;
    assert!(invalid_config.validate().is_err());
    
    // Reset and test invalid log level
    invalid_config = Config::default();
    invalid_config.monitoring.log_level = "invalid".to_string();
    assert!(invalid_config.validate().is_err());
}

#[test]
fn test_config_file_io() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config.toml");
    
    let original_config = Config::default();
    
    // Save and load config
    original_config.save_to_file(&config_path).unwrap();
    let loaded_config = Config::from_file(&config_path).unwrap();
    
    // Verify key fields match
    assert_eq!(original_config.engine.product_ids, loaded_config.engine.product_ids);
    assert_eq!(original_config.network.listen_port, loaded_config.network.listen_port);
    assert_eq!(original_config.performance.worker_threads, loaded_config.performance.worker_threads);
}

#[test]
fn test_product_specific_config() {
    let config = Config::default();
    
    // Should have BTC-USD config
    let btc_config = config.get_product_config(&"BTC-USD".to_string());
    assert!(btc_config.is_some());
    assert_eq!(btc_config.unwrap().tick_size, 100);
    
    // Should not have invalid product config
    let invalid_config = config.get_product_config(&"INVALID".to_string());
    assert!(invalid_config.is_none());
}

// ============================================================================
// Order Validation Tests
// ============================================================================

#[tokio::test]
async fn test_order_validation() {
    let mut config = Config::default();
    config.engine.wal_path = None;
    let engine = MatchingEngine::new(config).await.unwrap();
    
    // Test zero quantity
    let zero_qty_order = Order {
        id: uuid::Uuid::new_v4(),
        product_id: "BTC-USD".to_string(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: 50000,
        quantity: 0, // Invalid
        filled_quantity: 0,
        time_in_force: TimeInForce::GTC,
        submit_time: current_timestamp_ns(),
        client_id: None,
        metadata: OrderMetadata::default(),
        hidden_quantity: 0,
    };
    
    let result = engine.submit_order(zero_qty_order).await;
    assert!(matches!(result, Err(MatcherError::InvalidOrder(_))));
    
    // Test zero price for limit order
    let zero_price_order = Order {
        id: uuid::Uuid::new_v4(),
        product_id: "BTC-USD".to_string(),
        side: Side::Buy,
        order_type: OrderType::Limit,
        price: 0, // Invalid for limit order
        quantity: 100,
        filled_quantity: 0,
        time_in_force: TimeInForce::GTC,
        submit_time: current_timestamp_ns(),
        client_id: None,
        metadata: OrderMetadata::default(),
        hidden_quantity: 0,
    };
    
    let result = engine.submit_order(zero_price_order).await;
    assert!(matches!(result, Err(MatcherError::InvalidOrder(_))));
    
    // Test unsupported product
    let invalid_product_order = Order::limit("INVALID-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let result = engine.submit_order(invalid_product_order).await;
    assert!(matches!(result, Err(MatcherError::ProductNotSupported { .. })));
}

// ============================================================================
// Statistics Tests
// ============================================================================

#[tokio::test]
async fn test_engine_statistics() {
    let mut config = Config::default();
    config.engine.wal_path = None;
    let engine = MatchingEngine::new(config).await.unwrap();
    
    let initial_stats = engine.get_stats();
    
    // Submit orders
    let order1 = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let order2 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 50, current_timestamp_ns() + 1000);
    
    engine.submit_order(order1).await.unwrap();
    let matches = engine.submit_order(order2).await.unwrap();
    
    let final_stats = engine.get_stats();
    
    // Check statistics
    assert_eq!(final_stats.orders_received, initial_stats.orders_received + 2);
    if !matches.is_empty() {
        assert!(final_stats.orders_matched > initial_stats.orders_matched);
        assert!(final_stats.trades_executed > initial_stats.trades_executed);
    }
}

// ============================================================================
// Price-Time Priority Tests
// ============================================================================

#[test]
fn test_price_time_priority() {
    let book = OrderBook::new("BTC-USD".to_string());
    let base_time = current_timestamp_ns();
    
    // Add orders with same price but different times
    let order1 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, base_time);
    let order2 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, base_time + 1000);
    let order3 = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, base_time + 2000);
    
    let id1 = order1.id;
    
    book.add_order(order1).unwrap();
    book.add_order(order2).unwrap();
    book.add_order(order3).unwrap();
    
    // Submit buy order that matches
    let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 150, base_time + 3000);
    let matches = book.match_order(buy_order, base_time + 3000);
    
    // Should match in time priority (earliest first)
    assert!(!matches.is_empty());
    assert_eq!(matches[0].sell_order_id, id1);
}

// ============================================================================
// Order Types Tests
// ============================================================================

#[test]
fn test_order_creation_methods() {
    let timestamp = current_timestamp_ns();
    
    // Market order
    let market = Order::market("BTC-USD".to_string(), Side::Buy, 100, timestamp);
    assert_eq!(market.price, 0);
    assert!(matches!(market.order_type, OrderType::Market));
    
    // Limit order
    let limit = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, timestamp);
    assert_eq!(limit.price, 50000);
    assert!(matches!(limit.order_type, OrderType::Limit));
    
    // Post-Only order
    let post_only = Order::post_only("BTC-USD".to_string(), Side::Buy, 49000, 100, timestamp);
    assert_eq!(post_only.price, 49000);
    assert!(matches!(post_only.order_type, OrderType::PostOnly));
    
    // Iceberg order
    let iceberg = Order::iceberg("BTC-USD".to_string(), Side::Sell, 51000, 1000, 100, timestamp);
    assert_eq!(iceberg.quantity, 100); // Visible quantity
    assert_eq!(iceberg.hidden_quantity, 900); // Hidden quantity
    assert!(matches!(iceberg.order_type, OrderType::Iceberg { visible_size: 100 }));
}

#[test]
fn test_order_helper_methods() {
    let mut order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    
    // Test remaining quantity
    assert_eq!(order.remaining_quantity(), 100);
    assert!(!order.is_filled());
    
    // Partially fill
    order.filled_quantity = 30;
    assert_eq!(order.remaining_quantity(), 70);
    assert!(!order.is_filled());
    
    // Fully fill
    order.filled_quantity = 100;
    assert_eq!(order.remaining_quantity(), 0);
    assert!(order.is_filled());
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_error_types() {
    // Test error creation and matching
    let config_error = MatcherError::Config("Test config error".to_string());
    assert!(matches!(config_error, MatcherError::Config(_)));
    
    let order_not_found = MatcherError::OrderNotFound { 
        order_id: uuid::Uuid::new_v4() 
    };
    assert!(matches!(order_not_found, MatcherError::OrderNotFound { .. }));
    
    let product_not_supported = MatcherError::ProductNotSupported { 
        product_id: "INVALID".to_string() 
    };
    assert!(matches!(product_not_supported, MatcherError::ProductNotSupported { .. }));
}

// ============================================================================
// Performance and Edge Cases
// ============================================================================

#[test]
fn test_large_order_book() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add many orders
    for i in 0..1000 {
        let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000 - i, 100, i);
        let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 51000 + i, 100, i + 10000);
        book.add_order(buy_order).unwrap();
        book.add_order(sell_order).unwrap();
    }
    
    // Should still work efficiently
    assert_eq!(book.best_bid(), Some(50000));
    assert_eq!(book.best_ask(), Some(51000));
    
    let (bids, asks) = book.snapshot(10);
    assert_eq!(bids.len(), 10);
    assert_eq!(asks.len(), 10);
}

#[tokio::test]
async fn test_concurrent_operations() {
    let mut config = Config::default();
    config.engine.wal_path = None;
    let engine = std::sync::Arc::new(MatchingEngine::new(config).await.unwrap());
    
    // Submit orders concurrently
    let mut handles = Vec::new();
    
    for i in 0..10 {
        let engine_clone = engine.clone();
        let handle = tokio::spawn(async move {
            let order = Order::limit(
                "BTC-USD".to_string(),
                if i % 2 == 0 { Side::Buy } else { Side::Sell },
                50000 + (i * 100),
                100,
                current_timestamp_ns() + i,
            );
            engine_clone.submit_order(order).await
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }
    
    let stats = engine.get_stats();
    assert_eq!(stats.orders_received, 10);
}