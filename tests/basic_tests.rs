use matcher::*;
use matcher::core::order_book::OrderBook;
use matcher::core::MatchingEngine;
use matcher::utils::current_timestamp_ns;

#[tokio::test]
async fn test_basic_order_submission() {
    let config = Config::default();
    let engine = MatchingEngine::new(config).await.unwrap();
    
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let matches = engine.submit_order(order).await.unwrap();
    
    // Should not match anything initially
    assert_eq!(matches.len(), 0);
}

#[tokio::test]
async fn test_basic_order_matching() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let engine = MatchingEngine::new(config).await.unwrap();
    
    // Submit sell order first
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, current_timestamp_ns());
    engine.submit_order(sell_order).await.unwrap();
    
    // Submit matching buy order
    let buy_order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 50, current_timestamp_ns() + 1000);
    let matches = engine.submit_order(buy_order).await.unwrap();
    
    // Should match
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].price, 50000);
    assert_eq!(matches[0].quantity, 50);
}

#[tokio::test]
async fn test_order_cancellation() {
    let config = Config::default();
    let engine = MatchingEngine::new(config).await.unwrap();
    
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    let order_id = order.id;
    let product_id = order.product_id.clone();
    
    engine.submit_order(order).await.unwrap();
    
    // Cancel the order
    let cancelled = engine.cancel_order(&product_id, order_id).await.unwrap();
    assert_eq!(cancelled.id, order_id);
}

#[test]
fn test_order_book_basic_operations() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Test empty book
    assert_eq!(book.best_bid(), None);
    assert_eq!(book.best_ask(), None);
    
    // Add an order
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    book.add_order(order).unwrap();
    
    // Check best bid
    assert_eq!(book.best_bid(), Some(50000));
    assert_eq!(book.best_ask(), None);
}

#[test]
fn test_post_only_order() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add a sell order
    let sell_order = Order::limit("BTC-USD".to_string(), Side::Sell, 50000, 100, current_timestamp_ns());
    book.add_order(sell_order).unwrap();
    
    // Try Post-Only buy order at same price (should be rejected)
    let post_only = Order::post_only("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns() + 1000);
    let matches = book.match_order(post_only, current_timestamp_ns());
    
    // Should be rejected (no matches)
    assert_eq!(matches.len(), 0);
}

#[test]
fn test_iceberg_order() {
    let book = OrderBook::new("BTC-USD".to_string());
    
    // Add iceberg order
    let iceberg = Order::iceberg("BTC-USD".to_string(), Side::Sell, 50000, 1000, 100, current_timestamp_ns());
    book.add_order(iceberg).unwrap();
    
    // Check snapshot shows only visible quantity
    let (_, asks) = book.snapshot(10);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].quantity, 100); // Only visible quantity
    assert_eq!(asks[0].price, 50000);
}

#[test]
fn test_config_validation() {
    let config = Config::default();
    assert!(config.validate().is_ok());
    
    // Test invalid config
    let mut invalid_config = Config::default();
    invalid_config.engine.product_ids.clear();
    assert!(invalid_config.validate().is_err());
}

#[test]
fn test_order_creation() {
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    
    assert_eq!(order.product_id, "BTC-USD");
    assert_eq!(order.side, Side::Buy);
    assert_eq!(order.price, 50000);
    assert_eq!(order.quantity, 100);
    assert_eq!(order.filled_quantity, 0);
    assert!(!order.is_filled());
    assert_eq!(order.remaining_quantity(), 100);
}

#[test]
fn test_market_order_creation() {
    let order = Order::market("BTC-USD".to_string(), Side::Buy, 100, current_timestamp_ns());
    
    assert_eq!(order.price, 0);
    assert!(matches!(order.order_type, OrderType::Market));
}

#[tokio::test]
async fn test_engine_statistics() {
    let mut config = Config::default();
    config.engine.wal_path = None; // Disable WAL for this test
    let engine = MatchingEngine::new(config).await.unwrap();
    
    let initial_stats = engine.get_stats();
    
    // Submit an order
    let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, current_timestamp_ns());
    engine.submit_order(order).await.unwrap();
    
    let final_stats = engine.get_stats();
    assert_eq!(final_stats.orders_received, initial_stats.orders_received + 1);
}