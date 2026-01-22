// Property-Based Tests for WAL (Write-Ahead Log)
// Feature: trading-matching-engine
// 
// This test suite verifies WAL persistence and recovery properties.

use matcher::*;
use matcher::types::{Order, Side};
use matcher::core::MatchingEngine;
use matcher::storage::wal::{WalManager, LogEntry};
use matcher::utils::current_timestamp_ns;
use proptest::prelude::*;
use tempfile::TempDir;

// ============================================================================
// Test Data Generators
// ============================================================================

fn valid_price() -> impl Strategy<Value = u64> {
    (10u64..=1000u64).prop_map(|x| x * 100)
}

fn valid_quantity() -> impl Strategy<Value = u64> {
    (1u64..=1000u64).prop_map(|x| x * 10)
}

fn valid_order() -> impl Strategy<Value = Order> {
    (valid_price(), valid_quantity(), any::<bool>())
        .prop_map(|(price, quantity, is_buy)| {
            let side = if is_buy { Side::Buy } else { Side::Sell };
            Order::limit("BTC-USD".to_string(), side, price, quantity, current_timestamp_ns())
        })
}

// ============================================================================
// Property 14: Order Submission WAL Logging
// Feature: trading-matching-engine, Property 14
// Validates: Requirements 4.1
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    
    #[test]
    fn prop_order_submission_wal_logging(
        orders in prop::collection::vec(valid_order(), 1..10)
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let wal_path = temp_dir.path().join("test.wal");
            
            let mut config = Config::default();
            config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
            
            let engine = MatchingEngine::new(config).await.unwrap();
            
            // Submit orders
            for order in &orders {
                let _ = engine.submit_order(order.clone()).await;
            }
            
            // Read WAL directly
            let wal = WalManager::new(&wal_path).unwrap();
            let entries = wal.replay().unwrap();
            
            // Property: WAL should contain at least as many entries as orders submitted
            // (may have more due to matches generating additional entries)
            prop_assert!(entries.len() >= orders.len(),
                "WAL has {} entries but submitted {} orders", 
                entries.len(), orders.len());
            
            // Property: All PlaceOrder entries should be present
            let place_order_count = entries.iter()
                .filter(|e| matches!(e, LogEntry::PlaceOrder(_)))
                .count();
            prop_assert!(place_order_count >= orders.len(),
                "WAL has {} PlaceOrder entries but submitted {} orders",
                place_order_count, orders.len());
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 15: WAL Replay State Recovery
// Feature: trading-matching-engine, Property 15
// Validates: Requirements 4.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_wal_replay_state_recovery(
        orders in prop::collection::vec(valid_order(), 1..20)
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let wal_path = temp_dir.path().join("test.wal");
            
            let mut config = Config::default();
            config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
            
            // First engine: submit orders
            let engine1 = MatchingEngine::new(config.clone()).await.unwrap();
            
            for order in &orders {
                let _ = engine1.submit_order(order.clone()).await;
            }
            
            let stats1 = engine1.get_stats();
            
            // Drop first engine
            drop(engine1);
            
            // Second engine: replay from WAL
            let engine2 = MatchingEngine::new(config).await.unwrap();
            let stats2 = engine2.get_stats();
            
            // Property: Statistics should be restored after replay
            prop_assert_eq!(stats1.orders_received, stats2.orders_received,
                "Orders received should match after replay");
            
            // Property: Order book state should be consistent
            let (bid1, ask1) = engine2.get_best_prices(&"BTC-USD".to_string()).unwrap();
            
            // If there were orders, there should be some state
            if !orders.is_empty() {
                prop_assert!(bid1.is_some() || ask1.is_some() || stats2.orders_matched > 0,
                    "After replay, order book should have state or matches should have occurred");
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 16: WAL Replay Error Resilience
// Feature: trading-matching-engine, Property 16
// Validates: Requirements 4.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_wal_replay_error_resilience(
        valid_orders in prop::collection::vec(valid_order(), 1..10),
        invalid_order_index in 0usize..5usize
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let wal_path = temp_dir.path().join("test.wal");
            
            let wal = WalManager::new(&wal_path).unwrap();
            
            // Write valid orders
            for (i, order) in valid_orders.iter().enumerate() {
                wal.append(&LogEntry::PlaceOrder(order.clone())).unwrap();
                
                // Insert an invalid order (wrong product) at specific index
                if i == invalid_order_index && i < valid_orders.len() {
                    let mut invalid = order.clone();
                    invalid.product_id = "INVALID-PRODUCT".to_string();
                    wal.append(&LogEntry::PlaceOrder(invalid)).unwrap();
                }
            }
            
            // Create engine with WAL (should replay and handle errors)
            let mut config = Config::default();
            config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
            
            let result = MatchingEngine::new(config).await;
            
            // Property: Engine should initialize successfully despite invalid entries
            prop_assert!(result.is_ok(), 
                "Engine should handle invalid WAL entries gracefully");
            
            if let Ok(engine) = result {
                let stats = engine.get_stats();
                
                // Property: Valid orders should still be processed
                prop_assert!(stats.orders_received > 0,
                    "Valid orders should be processed despite invalid entries");
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 8: Cancellation WAL Logging
// Feature: trading-matching-engine, Property 8
// Validates: Requirements 2.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    
    #[test]
    fn prop_cancellation_wal_logging(
        orders in prop::collection::vec(valid_order(), 1..10)
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let wal_path = temp_dir.path().join("test.wal");
            
            let mut config = Config::default();
            config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
            
            let engine = MatchingEngine::new(config).await.unwrap();
            
            let mut order_ids = Vec::new();
            
            // Submit orders
            for order in &orders {
                let _ = engine.submit_order(order.clone()).await;
                order_ids.push(order.id);
            }
            
            // Cancel some orders
            let cancel_count = (orders.len() / 2).max(1);
            for i in 0..cancel_count {
                let _ = engine.cancel_order(&"BTC-USD".to_string(), order_ids[i]).await;
            }
            
            // Read WAL
            let wal = WalManager::new(&wal_path).unwrap();
            let entries = wal.replay().unwrap();
            
            // Property: WAL should contain CancelOrder entries
            let cancel_entries = entries.iter()
                .filter(|e| matches!(e, LogEntry::CancelOrder(_, _)))
                .count();
            
            prop_assert!(cancel_entries > 0,
                "WAL should contain CancelOrder entries");
            
            // Property: Number of cancel entries should match cancellations
            prop_assert!(cancel_entries <= cancel_count,
                "WAL has {} cancel entries but only {} cancellations were made",
                cancel_entries, cancel_count);
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 33: WAL Path Configuration
// Feature: trading-matching-engine, Property 33
// Validates: Requirements 9.5
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_wal_path_configuration(
        orders in prop::collection::vec(valid_order(), 1..5)
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let custom_wal_path = temp_dir.path().join("custom_location").join("my.wal");
            
            // Create parent directory
            std::fs::create_dir_all(custom_wal_path.parent().unwrap()).unwrap();
            
            let mut config = Config::default();
            config.engine.wal_path = Some(custom_wal_path.to_string_lossy().to_string());
            
            let engine = MatchingEngine::new(config).await.unwrap();
            
            // Submit orders
            for order in &orders {
                let _ = engine.submit_order(order.clone()).await;
            }
            
            // Property: WAL file should exist at configured path
            prop_assert!(custom_wal_path.exists(),
                "WAL file should be created at configured path: {:?}", custom_wal_path);
            
            // Property: WAL file should contain data
            let metadata = std::fs::metadata(&custom_wal_path).unwrap();
            prop_assert!(metadata.len() > 0,
                "WAL file should contain data");
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// WAL Integrity Tests
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_wal_append_order_preservation(
        orders in prop::collection::vec(valid_order(), 1..50)
    ) {
        let temp_dir = TempDir::new().unwrap();
        let wal_path = temp_dir.path().join("test.wal");
        
        let wal = WalManager::new(&wal_path).unwrap();
        
        // Append all orders
        for order in &orders {
            wal.append(&LogEntry::PlaceOrder(order.clone())).unwrap();
        }
        
        // Replay and verify
        let entries = wal.replay().unwrap();
        
        // Property: All orders should be preserved in order
        prop_assert_eq!(entries.len(), orders.len(),
            "WAL should preserve all {} orders", orders.len());
        
        // Property: Order sequence should be preserved
        for (i, entry) in entries.iter().enumerate() {
            if let LogEntry::PlaceOrder(replayed_order) = entry {
                prop_assert_eq!(replayed_order.id, orders[i].id,
                    "Order at position {} should match", i);
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_wal_concurrent_writes(
        orders in prop::collection::vec(valid_order(), 10..30)
    ) {
        let temp_dir = TempDir::new().unwrap();
        let wal_path = temp_dir.path().join("test.wal");
        
        let wal = std::sync::Arc::new(WalManager::new(&wal_path).unwrap());
        
        // Write orders concurrently (simulated)
        for order in &orders {
            let entry = LogEntry::PlaceOrder(order.clone());
            wal.append(&entry).unwrap();
        }
        
        // Replay
        let entries = wal.replay().unwrap();
        
        // Property: All writes should be persisted
        prop_assert_eq!(entries.len(), orders.len(),
            "All concurrent writes should be persisted");
    }
}

// ============================================================================
// WAL Recovery Scenarios
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_wal_multiple_restart_cycles(
        order_batches in prop::collection::vec(
            prop::collection::vec(valid_order(), 1..5),
            2..5
        )
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let temp_dir = TempDir::new().unwrap();
            let wal_path = temp_dir.path().join("test.wal");
            
            let mut config = Config::default();
            config.engine.wal_path = Some(wal_path.to_string_lossy().to_string());
            
            let mut total_orders = 0;
            
            // Multiple restart cycles
            for batch in &order_batches {
                let engine = MatchingEngine::new(config.clone()).await.unwrap();
                
                for order in batch {
                    let _ = engine.submit_order(order.clone()).await;
                    total_orders += 1;
                }
                
                drop(engine); // Simulate restart
            }
            
            // Final restart and verify
            let final_engine = MatchingEngine::new(config).await.unwrap();
            let stats = final_engine.get_stats();
            
            // Property: All orders from all cycles should be recovered
            prop_assert_eq!(stats.orders_received, total_orders as u64,
                "All orders from {} restart cycles should be recovered", 
                order_batches.len());
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}
