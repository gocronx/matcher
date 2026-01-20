use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use crate::types::{MatcherError, ProductId};

/// Main configuration structure for the matching engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Engine configuration
    pub engine: EngineConfig,
    
    /// Network configuration
    pub network: NetworkConfig,
    
    /// Performance tuning configuration
    pub performance: PerformanceConfig,
    
    /// Monitoring configuration
    pub monitoring: MonitoringConfig,
    
    /// Product-specific configurations
    pub products: HashMap<ProductId, ProductConfig>,
}

/// Engine-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    /// List of product IDs to support
    pub product_ids: Vec<ProductId>,
    
    /// Maximum number of orders per product
    pub max_orders_per_product: u32,
    
    /// Maximum order book depth to maintain in memory
    pub max_book_depth: u32,
    
    /// Enable risk management checks
    pub enable_risk_management: bool,
    
    /// Order ID generation strategy
    pub order_id_strategy: OrderIdStrategy,

    /// Path to the Write-Ahead Log file directory. If None, WAL is disabled.
    pub wal_path: Option<String>,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Port to listen on for incoming connections
    pub listen_port: u16,
    
    /// Multicast address for receiving orders
    pub multicast_addr: String,
    
    /// Broadcast address for sending match results
    pub broadcast_addr: String,
    
    /// Network buffer sizes
    pub buffer_size: usize,
    
    /// Enable TCP keepalive
    pub tcp_keepalive: bool,
    
    /// Socket timeout in milliseconds
    pub socket_timeout_ms: u64,
}

/// Performance tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Number of worker threads
    pub worker_threads: usize,
    
    /// Batch size for processing orders
    pub batch_size: u32,
    
    /// CPU frequency in GHz for high-resolution timing
    pub cpu_freq_ghz: f64,
    
    /// Enable NUMA awareness
    pub numa_aware: bool,
    
    /// Memory pre-allocation size
    pub memory_pool_size: usize,
    
    /// Enable lock-free algorithms where possible
    pub lock_free: bool,
    
    /// Use fast hash (ahash) instead of default
    pub use_fast_hash: bool,
    
    /// Enable object pool for orders
    pub use_object_pool: bool,
    
    /// Enable SmallVec optimization
    pub use_smallvec: bool,
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Enable Prometheus metrics
    pub enable_metrics: bool,
    
    /// Metrics HTTP server port
    pub metrics_port: u16,
    
    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,
    
    /// Enable structured logging
    pub structured_logging: bool,
    
    /// Health check endpoint port
    pub health_check_port: u16,
}

/// Product-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductConfig {
    /// Minimum price increment (tick size)
    pub tick_size: u64,
    
    /// Minimum quantity increment
    pub lot_size: u64,
    
    /// Maximum order size
    pub max_order_size: u64,
    
    /// Price precision (decimal places)
    pub price_precision: u8,
    
    /// Quantity precision (decimal places)
    pub quantity_precision: u8,
    
    /// Trading hours (if applicable)
    pub trading_hours: Option<TradingHours>,
}

/// Trading hours configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingHours {
    /// Market open time (UTC, format: "HH:MM")
    pub open: String,
    
    /// Market close time (UTC, format: "HH:MM")
    pub close: String,
    
    /// Trading days (0 = Sunday, 6 = Saturday)
    pub trading_days: Vec<u8>,
}

/// Order ID generation strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderIdStrategy {
    /// Use UUID v4 (random)
    UuidV4,
    
    /// Use sequential numbering
    Sequential,
    
    /// Use timestamp-based IDs
    Timestamp,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, MatcherError> {
        let content = fs::read_to_string(path)
            .map_err(|e| MatcherError::Config(format!("Failed to read config file: {}", e)))?;
        
        let config: Config = toml::from_str(&content)
            .map_err(|e| MatcherError::Config(format!("Failed to parse config: {}", e)))?;
        
        config.validate()?;
        Ok(config)
    }
    
    /// Create a default configuration
    pub fn default() -> Self {
        let mut products = HashMap::new();
        products.insert(
            "BTC-USD".to_string(),
            ProductConfig {
                tick_size: 100, // $0.01
                lot_size: 1,
                max_order_size: 1_000_000_000, // 10 BTC (in satoshis)
                price_precision: 2,
                quantity_precision: 8,
                trading_hours: None, // 24/7 trading
            },
        );
        
        Self {
            engine: EngineConfig {
                product_ids: vec!["BTC-USD".to_string()],
                max_orders_per_product: 1_000_000,
                max_book_depth: 10_000,
                enable_risk_management: true,
                order_id_strategy: OrderIdStrategy::UuidV4,
                wal_path: Some("data/wal".to_string()),
            },
            network: NetworkConfig {
                listen_port: 8080,
                multicast_addr: "239.0.0.1:5000".to_string(),
                broadcast_addr: "239.0.0.2:5001".to_string(),
                buffer_size: 65536,
                tcp_keepalive: true,
                socket_timeout_ms: 5000,
            },
            performance: PerformanceConfig {
                worker_threads: num_cpus::get(),
                batch_size: 100,
                cpu_freq_ghz: 3.0,
                numa_aware: false,
                memory_pool_size: 1024 * 1024 * 100, // 100MB
                lock_free: true,
                use_fast_hash: true,
                use_object_pool: true,
                use_smallvec: true,
            },
            monitoring: MonitoringConfig {
                enable_metrics: true,
                metrics_port: 9090,
                log_level: "info".to_string(),
                structured_logging: true,
                health_check_port: 8081,
            },
            products,
        }
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), MatcherError> {
        // Validate engine config
        if self.engine.product_ids.is_empty() {
            return Err(MatcherError::Config("No product IDs specified".to_string()));
        }
        
        if self.engine.max_orders_per_product == 0 {
            return Err(MatcherError::Config("max_orders_per_product must be > 0".to_string()));
        }
        
        // Validate network config
        if self.network.listen_port == 0 {
            return Err(MatcherError::Config("listen_port must be > 0".to_string()));
        }
        
        // Validate multicast addresses
        if self.network.multicast_addr.is_empty() {
            return Err(MatcherError::Config("multicast_addr cannot be empty".to_string()));
        }
        
        if self.network.broadcast_addr.is_empty() {
            return Err(MatcherError::Config("broadcast_addr cannot be empty".to_string()));
        }
        
        // Validate performance config
        if self.performance.worker_threads == 0 {
            return Err(MatcherError::Config("worker_threads must be > 0".to_string()));
        }
        
        if self.performance.batch_size == 0 {
            return Err(MatcherError::Config("batch_size must be > 0".to_string()));
        }
        
        if self.performance.cpu_freq_ghz <= 0.0 {
            return Err(MatcherError::Config("cpu_freq_ghz must be > 0".to_string()));
        }
        
        // Validate monitoring config
        let valid_log_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_log_levels.contains(&self.monitoring.log_level.as_str()) {
            return Err(MatcherError::Config(format!(
                "Invalid log level: {}. Must be one of: {:?}",
                self.monitoring.log_level, valid_log_levels
            )));
        }
        
        // Validate product configs
        for (product_id, product_config) in &self.products {
            if !self.engine.product_ids.contains(product_id) {
                return Err(MatcherError::Config(format!(
                    "Product {} has config but is not in engine.product_ids",
                    product_id
                )));
            }
            
            if product_config.tick_size == 0 {
                return Err(MatcherError::Config(format!(
                    "Product {} tick_size must be > 0",
                    product_id
                )));
            }
            
            if product_config.lot_size == 0 {
                return Err(MatcherError::Config(format!(
                    "Product {} lot_size must be > 0",
                    product_id
                )));
            }
        }
        
        Ok(())
    }
    
    /// Get configuration for a specific product
    pub fn get_product_config(&self, product_id: &ProductId) -> Option<&ProductConfig> {
        self.products.get(product_id)
    }
    
    /// Save configuration to a TOML file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), MatcherError> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| MatcherError::Config(format!("Failed to serialize config: {}", e)))?;
        
        fs::write(path, content)
            .map_err(|e| MatcherError::Config(format!("Failed to write config file: {}", e)))?;
        
        Ok(())
    }
}

// Add num_cpus as a dependency in Cargo.toml for the num_cpus::get() function
// For now, we'll use a simple fallback
impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            worker_threads: 4,
            batch_size: 100,
            cpu_freq_ghz: 3.0,
            numa_aware: false,
            memory_pool_size: 1024 * 1024 * 100,
            lock_free: true,
            use_fast_hash: true,      // 默认启用 ahash
            use_object_pool: true,    // 默认启用对象池
            use_smallvec: true,       // 默认启用 SmallVec
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.engine.product_ids.is_empty());
        assert!(config.engine.max_orders_per_product > 0);
        assert!(config.network.listen_port > 0);
        assert!(config.performance.worker_threads > 0);
    }

    #[test]
    fn test_config_validation() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_config_no_products() {
        let mut config = Config::default();
        config.engine.product_ids.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_config_zero_max_orders() {
        let mut config = Config::default();
        config.engine.max_orders_per_product = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_get_product_config() {
        let config = Config::default();
        let product_config = config.get_product_config(&"BTC-USD".to_string());
        assert!(product_config.is_some());
        
        let invalid = config.get_product_config(&"INVALID".to_string());
        assert!(invalid.is_none());
    }
}