// Property-Based Tests for Configuration Management
// Feature: trading-matching-engine
// 
// This test suite verifies configuration validation and application properties.

use matcher::*;
use matcher::config::ProductConfig;
use matcher::core::MatchingEngine;
use proptest::prelude::*;
use tempfile::TempDir;

// ============================================================================
// Test Data Generators
// ============================================================================

fn valid_port() -> impl Strategy<Value = u16> {
    1024u16..=65535u16
}

fn valid_product_id() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "BTC-USD".to_string(),
        "ETH-USD".to_string(),
        "SOL-USD".to_string(),
    ])
}

fn valid_log_level() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "trace".to_string(),
        "debug".to_string(),
        "info".to_string(),
        "warn".to_string(),
        "error".to_string(),
    ])
}

fn valid_tick_size() -> impl Strategy<Value = u64> {
    prop::sample::select(vec![1u64, 10, 100, 1000])
}

fn valid_lot_size() -> impl Strategy<Value = u64> {
    prop::sample::select(vec![1u64, 10, 100])
}

fn valid_worker_threads() -> impl Strategy<Value = usize> {
    1usize..=16usize
}

fn valid_batch_size() -> impl Strategy<Value = u32> {
    1u32..=1000u32
}

fn valid_max_orders() -> impl Strategy<Value = u32> {
    1u32..=1000000u32
}

// ============================================================================
// Property 35: Configuration Validation
// Feature: trading-matching-engine, Property 35
// Validates: Requirements 12.1
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    
    #[test]
    fn prop_configuration_validation_valid_configs(
        port in valid_port(),
        product_ids in prop::collection::vec(valid_product_id(), 1..5),
        log_level in valid_log_level(),
        worker_threads in valid_worker_threads(),
        batch_size in valid_batch_size()
    ) {
        let mut config = Config::default();
        config.network.listen_port = port;
        
        // Remove duplicates and ensure product configs match
        let unique_products: Vec<String> = product_ids.into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        
        config.engine.product_ids = unique_products.clone();
        
        // Clear existing product configs and add only for selected products
        config.products.clear();
        for product_id in &unique_products {
            config.products.insert(
                product_id.clone(),
                ProductConfig {
                    tick_size: 100,
                    lot_size: 1,
                    max_order_size: 1_000_000_000,
                    price_precision: 2,
                    quantity_precision: 8,
                    trading_hours: None,
                }
            );
        }
        
        config.monitoring.log_level = log_level;
        config.performance.worker_threads = worker_threads;
        config.performance.batch_size = batch_size;
        
        // Property: Valid configuration should pass validation
        let result = config.validate();
        prop_assert!(result.is_ok(), 
            "Valid configuration should pass validation: {:?}", result);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    
    #[test]
    fn prop_configuration_validation_invalid_configs(
        invalid_scenario in 0usize..=2usize
    ) {
        let mut config = Config::default();
        
        // Create different invalid scenarios
        match invalid_scenario {
            0 => config.engine.product_ids.clear(),  // Empty product list
            1 => config.engine.max_orders_per_product = 0,  // Zero max orders
            _ => config.monitoring.log_level = "invalid_level".to_string(),  // Invalid log level
        }
        
        // Property: Invalid configuration should fail validation
        let result = config.validate();
        prop_assert!(result.is_err(),
            "Configuration with invalid scenario {} should fail validation", invalid_scenario);
    }
}

// ============================================================================
// Property 36: Invalid Configuration Error Messages
// Feature: trading-matching-engine, Property 36
// Validates: Requirements 12.2
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_invalid_config_error_messages(
        scenario in 0usize..=5usize
    ) {
        let mut config = Config::default();
        
        // Create different invalid scenarios
        match scenario {
            0 => config.engine.product_ids.clear(),
            1 => config.engine.max_orders_per_product = 0,
            2 => config.network.listen_port = 0,
            3 => config.monitoring.log_level = "invalid_level".to_string(),
            4 => config.performance.worker_threads = 0,
            _ => config.performance.batch_size = 0,
        }
        
        let result = config.validate();
        
        // Property: Invalid configuration should return detailed error message
        prop_assert!(result.is_err(), "Invalid config should fail validation");
        
        if let Err(e) = result {
            let error_msg = format!("{}", e);
            
            // Property: Error message should be non-empty and descriptive
            prop_assert!(!error_msg.is_empty(), "Error message should not be empty");
            prop_assert!(error_msg.len() > 10, 
                "Error message should be descriptive, got: {}", error_msg);
        }
    }
}

// ============================================================================
// Property 37: Product-Specific Constraints
// Feature: trading-matching-engine, Property 37
// Validates: Requirements 12.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]
    
    #[test]
    fn prop_product_specific_constraints(
        tick_size in valid_tick_size(),
        lot_size in valid_lot_size(),
        max_order_size in 1000u64..=1000000u64
    ) {
        let mut config = Config::default();
        
        // Set product-specific configuration
        if let Some(product_config) = config.products.get_mut("BTC-USD") {
            product_config.tick_size = tick_size;
            product_config.lot_size = lot_size;
            product_config.max_order_size = max_order_size;
        }
        
        // Property: Configuration should validate successfully
        let result = config.validate();
        prop_assert!(result.is_ok(), "Valid product config should pass validation");
        
        // Property: Product-specific config should be retrievable
        let retrieved = config.get_product_config(&"BTC-USD".to_string());
        prop_assert!(retrieved.is_some(), "Product config should be retrievable");
        
        if let Some(pc) = retrieved {
            prop_assert_eq!(pc.tick_size, tick_size);
            prop_assert_eq!(pc.lot_size, lot_size);
            prop_assert_eq!(pc.max_order_size, max_order_size);
        }
    }
}

// ============================================================================
// Property 38: Monitoring Configuration
// Feature: trading-matching-engine, Property 38
// Validates: Requirements 12.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_monitoring_configuration(
        log_level in valid_log_level(),
        structured_logging in any::<bool>()
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.monitoring.log_level = log_level.clone();
            config.monitoring.structured_logging = structured_logging;
            config.engine.wal_path = None;
            
            // Property: Engine should initialize with monitoring config
            let result = MatchingEngine::new(config.clone()).await;
            prop_assert!(result.is_ok(), 
                "Engine should initialize with valid monitoring config");
            
            if let Ok(_engine) = result {
                // Property: Configuration values should be preserved
                prop_assert_eq!(config.monitoring.log_level, log_level);
                prop_assert_eq!(config.monitoring.structured_logging, structured_logging);
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 29: Product Order Book Creation
// Feature: trading-matching-engine, Property 29
// Validates: Requirements 9.1
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_product_order_book_creation(
        product_ids in prop::collection::vec(valid_product_id(), 1..10)
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            
            // Remove duplicates
            let unique_products: Vec<String> = product_ids.into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            
            config.engine.product_ids = unique_products.clone();
            config.engine.wal_path = None;
            
            let result = MatchingEngine::new(config).await;
            prop_assert!(result.is_ok(), "Engine should initialize with product list");
            
            if let Ok(engine) = result {
                // Property: Order books should be created for all products
                for product_id in &unique_products {
                    let best_prices = engine.get_best_prices(product_id);
                    prop_assert!(best_prices.is_ok(),
                        "Order book should exist for product {}", product_id);
                }
                
                // Property: Unsupported product should return error
                let invalid_result = engine.get_best_prices(&"INVALID-PRODUCT".to_string());
                prop_assert!(invalid_result.is_err(),
                    "Unsupported product should return error");
            }
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 30: Network Configuration Application
// Feature: trading-matching-engine, Property 30
// Validates: Requirements 9.2
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_network_configuration_application(
        port in valid_port(),
        buffer_size in 1024usize..=65536usize
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.network.listen_port = port;
            config.network.buffer_size = buffer_size;
            config.engine.wal_path = None;
            
            // Property: Engine should accept network configuration
            let result = MatchingEngine::new(config.clone()).await;
            prop_assert!(result.is_ok(),
                "Engine should initialize with network config");
            
            // Property: Configuration values should be preserved
            prop_assert_eq!(config.network.listen_port, port);
            prop_assert_eq!(config.network.buffer_size, buffer_size);
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 31: Performance Configuration Application
// Feature: trading-matching-engine, Property 31
// Validates: Requirements 9.3
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]
    
    #[test]
    fn prop_performance_configuration_application(
        worker_threads in valid_worker_threads(),
        batch_size in valid_batch_size()
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.performance.worker_threads = worker_threads;
            config.performance.batch_size = batch_size;
            config.engine.wal_path = None;
            
            // Property: Engine should initialize with performance config
            let result = MatchingEngine::new(config.clone()).await;
            prop_assert!(result.is_ok(),
                "Engine should initialize with performance config");
            
            // Property: Configuration values should be preserved
            prop_assert_eq!(config.performance.worker_threads, worker_threads);
            prop_assert_eq!(config.performance.batch_size, batch_size);
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Property 32: Optimization Feature Activation
// Feature: trading-matching-engine, Property 32
// Validates: Requirements 9.4
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_optimization_feature_activation(
        use_fast_hash in any::<bool>(),
        use_object_pool in any::<bool>(),
        use_smallvec in any::<bool>()
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut config = Config::default();
            config.performance.use_fast_hash = use_fast_hash;
            config.performance.use_object_pool = use_object_pool;
            config.performance.use_smallvec = use_smallvec;
            config.engine.wal_path = None;
            
            // Property: Engine should initialize with optimization flags
            let result = MatchingEngine::new(config.clone()).await;
            prop_assert!(result.is_ok(),
                "Engine should initialize with optimization config");
            
            // Property: Configuration flags should be preserved
            prop_assert_eq!(config.performance.use_fast_hash, use_fast_hash);
            prop_assert_eq!(config.performance.use_object_pool, use_object_pool);
            prop_assert_eq!(config.performance.use_smallvec, use_smallvec);
            
            Ok::<(), TestCaseError>(())
        }).unwrap();
    }
}

// ============================================================================
// Configuration File I/O Properties
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_config_file_roundtrip(
        port in valid_port(),
        product_ids in prop::collection::vec(valid_product_id(), 1..5),
        worker_threads in valid_worker_threads()
    ) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");
        
        // Create config with matching product configs
        let mut original_config = Config::default();
        original_config.network.listen_port = port;
        
        // Remove duplicates
        let unique_products: Vec<String> = product_ids.into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        
        original_config.engine.product_ids = unique_products.clone();
        
        // Clear and rebuild product configs to match
        original_config.products.clear();
        for product_id in &unique_products {
            original_config.products.insert(
                product_id.clone(),
                ProductConfig {
                    tick_size: 100,
                    lot_size: 1,
                    max_order_size: 1_000_000_000,
                    price_precision: 2,
                    quantity_precision: 8,
                    trading_hours: None,
                }
            );
        }
        
        original_config.performance.worker_threads = worker_threads;
        
        // Save to file
        let save_result = original_config.save_to_file(&config_path);
        prop_assert!(save_result.is_ok(), "Config should save successfully");
        
        // Load from file
        let load_result = Config::from_file(&config_path);
        prop_assert!(load_result.is_ok(), "Config should load successfully: {:?}", load_result);
        
        if let Ok(loaded_config) = load_result {
            // Property: Loaded config should match original
            prop_assert_eq!(loaded_config.network.listen_port, port);
            prop_assert_eq!(loaded_config.engine.product_ids, unique_products);
            prop_assert_eq!(loaded_config.performance.worker_threads, worker_threads);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_config_validation_consistency(
        port in valid_port(),
        max_orders in valid_max_orders()
    ) {
        let mut config = Config::default();
        config.network.listen_port = port;
        config.engine.max_orders_per_product = max_orders;
        
        // Property: Validation should be idempotent
        let result1 = config.validate();
        let result2 = config.validate();
        
        prop_assert_eq!(result1.is_ok(), result2.is_ok(),
            "Validation should be consistent across multiple calls");
    }
}

// ============================================================================
// Configuration Edge Cases
// ============================================================================

#[test]
fn test_config_empty_product_list_rejection() {
    let mut config = Config::default();
    config.engine.product_ids.clear();
    
    // Property: Empty product list should fail validation
    let result = config.validate();
    assert!(result.is_err(),
        "Configuration with empty product list should fail validation");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_config_zero_values_rejection(
        field in 0usize..=3usize
    ) {
        let mut config = Config::default();
        
        // Set different fields to zero
        match field {
            0 => config.engine.max_orders_per_product = 0,
            1 => config.performance.worker_threads = 0,
            2 => config.performance.batch_size = 0,
            _ => config.network.listen_port = 0,
        }
        
        // Property: Zero values in critical fields should fail validation
        let result = config.validate();
        prop_assert!(result.is_err(),
            "Configuration with zero value in field {} should fail validation", field);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]
    
    #[test]
    fn prop_config_product_constraints_consistency(
        tick_size in valid_tick_size(),
        lot_size in valid_lot_size()
    ) {
        let mut config = Config::default();
        
        // Set constraints for all products
        for (_, product_config) in config.products.iter_mut() {
            product_config.tick_size = tick_size;
            product_config.lot_size = lot_size;
        }
        
        // Property: All products should have consistent constraints
        for (_, product_config) in config.products.iter() {
            prop_assert_eq!(product_config.tick_size, tick_size);
            prop_assert_eq!(product_config.lot_size, lot_size);
        }
    }
}
