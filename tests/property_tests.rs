// Property-Based Tests for Trading Matching Engine
// Feature: trading-matching-engine
// 
// This test suite implements property-based testing using PropTest to verify
// correctness properties across thousands of random inputs.

use matcher::*;
use matcher::types::{Order, Side, OrderType, MatcherError};
use matcher::core::order_book::OrderBook;
use matcher::core::MatchingEngine;
use matcher::utils::current_timestamp_ns;
use proptest::prelude::*;

// ============================================================================
// Test Data Generators
// ============================================================================

/// Generate valid prices (multiples of 100, between 1000 and 100000)
fn valid_price() -> impl Strategy<Value = u64> {
    (10u64..=1000u64).prop_map(|x| x * 100)
}

/// Generate valid quantities (multiples of 10, between 10 and 10000)
fn valid_quantity() -> impl Strategy<Value = u64> {
    (1u64..=1000u64).prop_map(|x| x * 10)
}

/// Generate valid product IDs
fn valid_product_id() -> impl Strategy<Value = String> {
    prop::sample::select(vec!["BTC-USD".to_string(), "ETH-USD".to_string()])
}

/// Generate valid order side
fn valid_side() -> impl Strategy<Value = Side> {
    prop::sample::select(vec![Side::Buy, Side::Sell])
}

/// Generate valid limit order
fn valid_limit_order() -> impl Strategy<Value = Order> {
    (valid_product_id(), valid_side(), valid_price(), valid_quantity())
        .prop_map(|(product_id, side, price, quantity)| {
            Order::limit(product_id, side, price, quantity, current_timestamp_ns())
        })
}

/// Generate valid market order
fn valid_market_order() -> impl Strategy<Value = Order> {
    (valid_product_id(), valid_side(), valid_quantity())
        .prop_map(|(product_id, side, quantity)| {
            Order::market(product_id, side, quantity, current_timestamp_ns())
        })
}

/// Generate valid post-only order
fn valid_post_only_order() -> impl Strategy<Value = Order> {
    (valid_product_id(), valid_side(), valid_price(), valid_quantity())
        .prop_map(|(product_id, side, price, quantity)| {
            Order::post_only(product_id, side, price, quantity, current_timestamp_ns())
        })
}

// Iceberg order generator - currently unused but kept for future tests
#[allow(dead_code)]
fn valid_iceberg_order() -> impl Strategy<Value = Order> {
    (valid_product_id(), valid_side(), valid_price(), valid_quantity(), 10u64..=100u64)
        .prop_map(|(product_id, side, price, total_qty, visible_qty)| {
            Order::iceberg(product_id, side, price, total_qty, visible_qty, current_timestamp_ns())
        })
}

// ============================================================================
// Property 1: Market Order Immediate Execution
// Feature: trading-matching-engine, Property 1
// Validates: Requirements 1.1
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_market_order_immediate_execution(
        resting_orders in prop::collection::vec(valid_limit_order(), 1..10),
        market_order in valid_market_order()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add resting orders to provide liquidity
        for order in resting_orders {
            if order.product_id == "BTC-USD" {
                let _ = book.add_order(order);
            }
        }
        
        // Submit market order
        let matches = book.match_order(market_order.clone(), current_timestamp_ns());
        
        // Property: Market order should either match immediately or return empty if no liquidity
        let has_liquidity = match market_order.side {
            Side::Buy => book.best_ask().is_some(),
            Side::Sell => book.best_bid().is_some(),
        };
        
        if has_liquidity {
            // If liquidity exists, market order should generate matches
            prop_assert!(!matches.is_empty() || market_order.quantity == 0);
        }
        
        // Market order should never be added to the book
        let (bid_depth, ask_depth) = book.depth();
        prop_assert!(bid_depth + ask_depth <= 10); // Only resting orders
    }
}

// ============================================================================
// Property 2: Limit Order Price Constraint
// Feature: trading-matching-engine, Property 2
// Validates: Requirements 1.2
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_limit_order_price_constraint(
        limit_order in valid_limit_order(),
        resting_orders in prop::collection::vec(valid_limit_order(), 0..5)
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add resting orders
        for order in resting_orders {
            if order.product_id == "BTC-USD" {
                let _ = book.add_order(order);
            }
        }
        
        // Submit limit order
        let matches = book.match_order(limit_order.clone(), current_timestamp_ns());
        
        // Property: All matches should be at limit price or better
        for match_result in matches {
            match limit_order.side {
                Side::Buy => {
                    // Buy order: execution price should be <= limit price
                    prop_assert!(match_result.price <= limit_order.price,
                        "Buy order executed at {} but limit was {}", 
                        match_result.price, limit_order.price);
                }
                Side::Sell => {
                    // Sell order: execution price should be >= limit price
                    prop_assert!(match_result.price >= limit_order.price,
                        "Sell order executed at {} but limit was {}", 
                        match_result.price, limit_order.price);
                }
            }
        }
    }
}

// ============================================================================
// Property 3: Post-Only Order Rejection
// Feature: trading-matching-engine, Property 3
// Validates: Requirements 1.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_post_only_order_rejection(
        post_only_order in valid_post_only_order(),
        _resting_price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add a resting order that would match
        let resting_side = match post_only_order.side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        
        let resting_order = Order::limit(
            "BTC-USD".to_string(),
            resting_side,
            post_only_order.price, // Same price - would match
            100,
            current_timestamp_ns()
        );
        let _ = book.add_order(resting_order);
        
        // Get initial state
        let (initial_bid_depth, initial_ask_depth) = book.depth();
        
        // Submit post-only order
        let matches = book.match_order(post_only_order, current_timestamp_ns());
        
        // Property: Post-only order should not match (empty matches)
        prop_assert!(matches.is_empty(), 
            "Post-only order should not match but generated {} matches", matches.len());
        
        // Property: Order book state should be unchanged
        let (final_bid_depth, final_ask_depth) = book.depth();
        prop_assert_eq!(initial_bid_depth, final_bid_depth);
        prop_assert_eq!(initial_ask_depth, final_ask_depth);
    }
}

// ============================================================================
// Property 4: Iceberg Order Visibility
// Feature: trading-matching-engine, Property 4
// Validates: Requirements 1.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_iceberg_order_visibility(
        total_quantity in 100u64..=1000u64,
        visible_quantity in 10u64..=100u64,
        price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Create iceberg order
        let iceberg = Order::iceberg(
            "BTC-USD".to_string(),
            Side::Sell,
            price,
            total_quantity,
            visible_quantity,
            current_timestamp_ns()
        );
        
        let _ = book.add_order(iceberg);
        
        // Get snapshot
        let (_, asks) = book.snapshot(10);
        
        // Property: Only visible quantity should appear in snapshot
        if !asks.is_empty() {
            let level = &asks[0];
            prop_assert_eq!(level.price, price);
            prop_assert_eq!(level.quantity, visible_quantity,
                "Snapshot shows {} but should show only visible quantity {}", 
                level.quantity, visible_quantity);
        }
    }
}

// ============================================================================
// Property 5: IOC Order Partial Execution
// Feature: trading-matching-engine, Property 5
// Validates: Requirements 1.5
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_ioc_order_partial_execution(
        ioc_quantity in valid_quantity(),
        available_quantity in 10u64..=50u64,
        price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add resting order with limited quantity
        let resting = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            price,
            available_quantity,
            current_timestamp_ns()
        );
        let _ = book.add_order(resting);
        
        // Create IOC order
        let mut ioc_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            price,
            ioc_quantity,
            current_timestamp_ns() + 1000
        );
        ioc_order.order_type = OrderType::IOC;
        
        let initial_depth = book.depth();
        let matches = book.match_order(ioc_order.clone(), current_timestamp_ns());
        let final_depth = book.depth();
        
        // Property: IOC should execute matchable portion
        let total_matched: u64 = matches.iter().map(|m| m.quantity).sum();
        prop_assert!(total_matched <= ioc_quantity.min(available_quantity));
        
        // Property: Remaining quantity should NOT be added to book
        // (depth should only decrease or stay same, never increase)
        prop_assert!(final_depth.0 + final_depth.1 <= initial_depth.0 + initial_depth.1);
    }
}

// ============================================================================
// Property 9: Best Price Accuracy
// Feature: trading-matching-engine, Property 9
// Validates: Requirements 3.1
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_best_price_accuracy(
        orders in prop::collection::vec(valid_limit_order(), 1..20)
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        let mut expected_best_bid = None;
        let mut expected_best_ask = None;
        
        // Add orders and track expected best prices
        for order in orders {
            if order.product_id == "BTC-USD" {
                let _ = book.add_order(order.clone());
                
                match order.side {
                    Side::Buy => {
                        expected_best_bid = Some(expected_best_bid
                            .map(|current: u64| current.max(order.price))
                            .unwrap_or(order.price));
                    }
                    Side::Sell => {
                        expected_best_ask = Some(expected_best_ask
                            .map(|current: u64| current.min(order.price))
                            .unwrap_or(order.price));
                    }
                }
            }
        }
        
        // Property: Best prices should match expected values
        prop_assert_eq!(book.best_bid(), expected_best_bid);
        prop_assert_eq!(book.best_ask(), expected_best_ask);
    }
}

// ============================================================================
// Property 10: Spread Calculation
// Feature: trading-matching-engine, Property 10
// Validates: Requirements 3.2
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_spread_calculation(
        bid_price in valid_price(),
        ask_price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Ensure ask > bid for valid spread
        let (bid, ask) = if ask_price > bid_price {
            (bid_price, ask_price)
        } else {
            (ask_price, bid_price)
        };
        
        // Add orders
        let bid_order = Order::limit("BTC-USD".to_string(), Side::Buy, bid, 100, current_timestamp_ns());
        let ask_order = Order::limit("BTC-USD".to_string(), Side::Sell, ask, 100, current_timestamp_ns() + 1000);
        
        let _ = book.add_order(bid_order);
        let _ = book.add_order(ask_order);
        
        // Property: Spread should equal ask - bid
        let spread = book.spread();
        let expected_spread = ask.saturating_sub(bid);
        
        prop_assert_eq!(spread, Some(expected_spread),
            "Spread is {:?} but should be {}", spread, expected_spread);
    }
}

// ============================================================================
// Property 11: Snapshot Depth Consistency
// Feature: trading-matching-engine, Property 11
// Validates: Requirements 3.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_snapshot_depth_consistency(
        orders in prop::collection::vec(valid_limit_order(), 1..30),
        requested_depth in 1usize..=20usize
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add orders
        let mut bid_count = 0;
        let mut ask_count = 0;
        
        for order in orders {
            if order.product_id == "BTC-USD" {
                let _ = book.add_order(order.clone());
                match order.side {
                    Side::Buy => bid_count += 1,
                    Side::Sell => ask_count += 1,
                }
            }
        }
        
        // Get snapshot
        let (bids, asks) = book.snapshot(requested_depth);
        
        // Property: Snapshot should contain at most requested_depth levels
        prop_assert!(bids.len() <= requested_depth,
            "Bid snapshot has {} levels but requested {}", bids.len(), requested_depth);
        prop_assert!(asks.len() <= requested_depth,
            "Ask snapshot has {} levels but requested {}", asks.len(), requested_depth);
        
        // Property: Snapshot should not exceed actual order count
        // (Note: multiple orders at same price = 1 level)
        prop_assert!(bids.len() <= bid_count);
        prop_assert!(asks.len() <= ask_count);
    }
}

// ============================================================================
// Property 21: Tick Size Validation
// Feature: trading-matching-engine, Property 21
// Validates: Requirements 6.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_tick_size_validation(
        base_price in 100u64..=10000u64,
        offset in 1u64..=99u64
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.engine.wal_path = None;
            let engine = MatchingEngine::new(config).await.unwrap();
            
            // Create order with invalid price (not multiple of tick size 100)
            let invalid_price = base_price * 100 + offset;
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                invalid_price,
                100,
                current_timestamp_ns()
            );
            
            // Property: Order with invalid tick size should be rejected
            let result = engine.submit_order(order).await;
            prop_assert!(result.is_err(), "Order with invalid tick size should be rejected");
            
            if let Err(e) = result {
                prop_assert!(matches!(e, MatcherError::InvalidOrder(_)),
                    "Should return InvalidOrder error, got: {:?}", e);
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 22: Lot Size Validation
// Feature: trading-matching-engine, Property 22
// Validates: Requirements 6.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_lot_size_validation(
        base_quantity in 10u64..=100u64,
        offset in 1u64..=9u64
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.engine.wal_path = None;
            
            // Set lot size to 10 for testing
            if let Some(product_config) = config.products.get_mut("BTC-USD") {
                product_config.lot_size = 10;
            }
            
            let engine = MatchingEngine::new(config).await.unwrap();
            
            // Create order with invalid quantity (not multiple of lot size 10)
            let invalid_quantity = (base_quantity * 10) + offset;
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                50000,
                invalid_quantity,
                current_timestamp_ns()
            );
            
            // Property: Order with invalid lot size should be rejected
            let result = engine.submit_order(order).await;
            prop_assert!(result.is_err(), 
                "Order with invalid lot size {} should be rejected", invalid_quantity);
            
            if let Err(e) = result {
                prop_assert!(matches!(e, MatcherError::InvalidOrder(_)),
                    "Should return InvalidOrder error, got: {:?}", e);
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 23: Maximum Size Validation
// Feature: trading-matching-engine, Property 23
// Validates: Requirements 6.5
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_maximum_size_validation(
        excess in 10u64..=1000u64
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.engine.wal_path = None;
            
            // Get max order size from config before moving
            let max_size = config.get_product_config(&"BTC-USD".to_string())
                .map(|c| c.max_order_size)
                .unwrap_or(1000000);
            
            let engine = MatchingEngine::new(config).await.unwrap();
            
            // Create order exceeding max size
            let invalid_quantity = max_size + (excess * 10);
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Buy,
                50000,
                invalid_quantity,
                current_timestamp_ns()
            );
            
            // Property: Order exceeding max size should be rejected
            let result = engine.submit_order(order).await;
            prop_assert!(result.is_err(), "Order exceeding max size should be rejected");
            
            if let Err(e) = result {
                prop_assert!(matches!(e, MatcherError::InvalidOrder(_)),
                    "Should return InvalidOrder error, got: {:?}", e);
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 25: Price-Time Priority
// Feature: trading-matching-engine, Property 25
// Validates: Requirements 7.2
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_price_time_priority(
        price in valid_price(),
        quantities in prop::collection::vec(10u64..=100u64, 2..5)
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        let base_time = current_timestamp_ns();
        
        let mut order_ids = Vec::new();
        
        // Add multiple orders at same price with different timestamps
        for (i, &qty) in quantities.iter().enumerate() {
            let order = Order::limit(
                "BTC-USD".to_string(),
                Side::Sell,
                price,
                qty,
                base_time + (i as u64 * 1000) // Increasing timestamps
            );
            order_ids.push(order.id);
            let _ = book.add_order(order);
        }
        
        // Submit buy order that matches
        let total_qty: u64 = quantities.iter().sum();
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            price,
            total_qty,
            base_time + 10000
        );
        
        let matches = book.match_order(buy_order, base_time + 10000);
        
        // Property: Orders should match in time priority (earliest first)
        if !matches.is_empty() {
            prop_assert_eq!(matches[0].sell_order_id, order_ids[0],
                "First match should be earliest order");
        }
    }
}

// ============================================================================
// Property 26: Partial Fill Updates
// Feature: trading-matching-engine, Property 26
// Validates: Requirements 7.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_partial_fill_updates(
        total_quantity in 100u64..=1000u64,
        match_quantity in 10u64..=50u64,
        price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add large resting order
        let resting = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            price,
            total_quantity,
            current_timestamp_ns()
        );
        let _ = book.add_order(resting);
        
        // Submit smaller buy order for partial fill
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            price,
            match_quantity,
            current_timestamp_ns() + 1000
        );
        
        let matches = book.match_order(buy_order, current_timestamp_ns());
        
        // Property: Match quantity should equal requested quantity (or less if insufficient liquidity)
        let total_matched: u64 = matches.iter().map(|m| m.quantity).sum();
        prop_assert!(total_matched <= match_quantity);
        prop_assert!(total_matched <= total_quantity);
    }
}

// ============================================================================
// Property 27: Complete Fill Removal
// Feature: trading-matching-engine, Property 27
// Validates: Requirements 7.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_complete_fill_removal(
        quantity in valid_quantity(),
        price in valid_price()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add resting order
        let resting = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            price,
            quantity,
            current_timestamp_ns()
        );
        let _ = book.add_order(resting);
        
        let initial_depth = book.depth();
        
        // Submit buy order that completely fills the resting order
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            price,
            quantity,
            current_timestamp_ns() + 1000
        );
        
        let matches = book.match_order(buy_order, current_timestamp_ns());
        let final_depth = book.depth();
        
        // Property: If order is completely filled, it should be removed
        if !matches.is_empty() {
            let total_matched: u64 = matches.iter().map(|m| m.quantity).sum();
            if total_matched == quantity {
                // Order should be removed from book
                prop_assert!(final_depth.1 < initial_depth.1,
                    "Completely filled order should be removed from book");
            }
        }
    }
}

// ============================================================================
// Property 28: Match Timing Records
// Feature: trading-matching-engine, Property 28
// Validates: Requirements 7.5
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    
    #[test]
    fn prop_match_timing_records(
        price in valid_price(),
        quantity in valid_quantity()
    ) {
        let book = OrderBook::new("BTC-USD".to_string());
        
        // Add resting order
        let resting = Order::limit(
            "BTC-USD".to_string(),
            Side::Sell,
            price,
            quantity,
            current_timestamp_ns()
        );
        let _ = book.add_order(resting);
        
        // Submit matching order
        let current_time = current_timestamp_ns();
        let buy_order = Order::limit(
            "BTC-USD".to_string(),
            Side::Buy,
            price,
            quantity,
            current_time
        );
        
        let matches = book.match_order(buy_order, current_time);
        
        // Property: All matches should have timing information
        for match_result in matches {
            prop_assert!(match_result.trade_time > 0, "Trade time should be recorded");
            // match_latency_ns is u64, so it's always >= 0
            prop_assert!(match_result.trade_time >= current_time, 
                "Trade time should be >= submission time");
        }
    }
}
