use crate::config::Config;
use crate::core::order_book::OrderBook;
use crate::types::{Order, OrderId, MatchResult, ProductId, MatcherError, EngineStats, BookLevel};
use crate::utils::{Metrics, HighResTimer, current_timestamp_ns};
use crate::storage::wal::{WalManager, LogEntry};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, debug, warn};

/// Main matching engine that coordinates order processing
/// 
/// The engine manages multiple order books (one per product) and provides
/// a unified interface for order submission, cancellation, and matching.
pub struct MatchingEngine {
    config: Config,
    order_books: RwLock<HashMap<ProductId, Arc<OrderBook>>>,
    metrics: Arc<Metrics>,
    stats: RwLock<EngineStats>,
    start_time: u64,
    wal_manager: Option<Arc<WalManager>>,
}

impl MatchingEngine {
    /// Create a new matching engine with the given configuration
    pub async fn new(config: Config) -> Result<Self, MatcherError> {
        let metrics = Arc::new(Metrics::new()?);
        let start_time = current_timestamp_ns();
        
        // Create order books for each configured product
        let mut order_books = HashMap::new();
        for product_id in &config.engine.product_ids {
            let order_book = Arc::new(OrderBook::new(product_id.clone()));
            order_books.insert(product_id.clone(), order_book);
            info!("Created order book for product: {}", product_id);
        }

        // Initialize WAL
        let wal_manager = if let Some(path_str) = &config.engine.wal_path {
             let wm = WalManager::new(path_str).map_err(|e| MatcherError::Engine(format!("Failed to init WAL: {}", e)))?;
             Some(Arc::new(wm))
        } else {
             None
        };
        
        let engine = Self {
            config,
            order_books: RwLock::new(order_books),
            metrics,
            stats: RwLock::new(EngineStats {
                orders_received: 0,
                orders_matched: 0,
                trades_executed: 0,
                avg_match_latency_ns: 0,
                uptime_seconds: 0,
            }),
            start_time,
            wal_manager,
        };

        // Replay WAL if present
        if let Some(wal) = &engine.wal_manager {
             engine.replay_wal(wal)?;
        }
        
        info!("Matching engine initialized with {} products", 
              engine.config.engine.product_ids.len());
        
        Ok(engine)
    }
    
    /// Submit a new order to the matching engine
    pub async fn submit_order(&self, order: Order) -> Result<Vec<MatchResult>, MatcherError> {
        // Validate first to avoid writing invalid orders to WAL
        self.validate_order(&order)?;

        // Write to WAL if enabled
        if let Some(wal) = &self.wal_manager {
            wal.append(&LogEntry::PlaceOrder(order.clone()))
                .map_err(|e| MatcherError::Engine(format!("WAL write failed: {}", e)))?;
        }

        self.process_order_memory(order)
    }

    /// Internal method to process order in memory
    fn process_order_memory(&self, order: Order) -> Result<Vec<MatchResult>, MatcherError> {
        let timer = HighResTimer::start();
        
        // Validate the order
        self.validate_order(&order)?;
        
        // Get the appropriate order book
        let order_book = {
            let order_books = self.order_books.read();
            order_books.get(&order.product_id)
                .ok_or_else(|| MatcherError::ProductNotSupported { 
                    product_id: order.product_id.clone() 
                })?
                .clone()
        };
        
        // Record metrics
        self.metrics.record_order_received();
        
        // Attempt to match the order
        let current_time = current_timestamp_ns();
        let matches = order_book.match_order(order.clone(), current_time);
        
        // Record match metrics
        let match_latency = timer.elapsed_ns();
        self.metrics.record_match_latency(match_latency);
        
        if !matches.is_empty() {
            self.metrics.record_order_matched();
            
            // Update statistics
            {
                let mut stats = self.stats.write();
                stats.orders_matched += 1;
                stats.trades_executed += matches.len() as u64;
                
                // Update average latency (simple moving average)
                stats.avg_match_latency_ns = 
                    (stats.avg_match_latency_ns + match_latency) / 2;
            }
            
            debug!("Order {} generated {} matches", order.id, matches.len());
        }
        
        // Update order statistics
        {
            let mut stats = self.stats.write();
            stats.orders_received += 1;
            stats.uptime_seconds = (current_time - self.start_time) / 1_000_000_000;
        }
        
        Ok(matches)
    }
    
    /// Cancel an existing order
    pub async fn cancel_order(&self, product_id: &ProductId, order_id: OrderId) -> Result<Order, MatcherError> {
        // Write to WAL if enabled
        if let Some(wal) = &self.wal_manager {
            wal.append(&LogEntry::CancelOrder(order_id, product_id.clone()))
                .map_err(|e| MatcherError::Engine(format!("WAL write failed: {}", e)))?;
        }

        self.process_cancel_memory(product_id, order_id)
    }

    /// Internal method to cancel order in memory
    fn process_cancel_memory(&self, product_id: &ProductId, order_id: OrderId) -> Result<Order, MatcherError> {
        // Get the appropriate order book
        let order_book = {
            let order_books = self.order_books.read();
            order_books.get(product_id)
                .ok_or_else(|| MatcherError::ProductNotSupported { 
                    product_id: product_id.clone() 
                })?
                .clone()
        };
        
        // Remove the order from the book
        let cancelled_order = order_book.remove_order(order_id)
            .map_err(|_e| MatcherError::OrderNotFound { order_id })?;
        
        self.metrics.record_order_cancelled();
        
        info!("Cancelled order {} for product {}", order_id, product_id);
        
        Ok(cancelled_order)
    }
    
    /// Get current engine statistics
    pub fn get_stats(&self) -> EngineStats {
        let mut stats = self.stats.read().clone();
        stats.uptime_seconds = (current_timestamp_ns() - self.start_time) / 1_000_000_000;
        stats
    }
    
    /// Get order book snapshot for a product
    pub fn get_order_book_snapshot(&self, product_id: &ProductId, depth: usize) -> Result<(Vec<BookLevel>, Vec<BookLevel>), MatcherError> {
        let order_books = self.order_books.read();
        let order_book = order_books.get(product_id)
            .ok_or_else(|| MatcherError::ProductNotSupported { 
                product_id: product_id.clone() 
            })?;
        
        Ok(order_book.snapshot(depth))
    }
    
    /// Get best bid and ask for a product
    pub fn get_best_prices(&self, product_id: &ProductId) -> Result<(Option<u64>, Option<u64>), MatcherError> {
        let order_books = self.order_books.read();
        let order_book = order_books.get(product_id)
            .ok_or_else(|| MatcherError::ProductNotSupported { 
                product_id: product_id.clone() 
            })?;
        
        Ok((order_book.best_bid(), order_book.best_ask()))
    }
    
    /// Get the spread for a product
    pub fn get_spread(&self, product_id: &ProductId) -> Result<Option<u64>, MatcherError> {
        let order_books = self.order_books.read();
        let order_book = order_books.get(product_id)
            .ok_or_else(|| MatcherError::ProductNotSupported { 
                product_id: product_id.clone() 
            })?;
        
        Ok(order_book.spread())
    }
    
    /// Get order book depth for a product
    pub fn get_depth(&self, product_id: &ProductId) -> Result<(usize, usize), MatcherError> {
        let order_books = self.order_books.read();
        let order_book = order_books.get(product_id)
            .ok_or_else(|| MatcherError::ProductNotSupported { 
                product_id: product_id.clone() 
            })?;
        
        Ok(order_book.depth())
    }
    
    /// Get metrics registry for Prometheus exposition
    pub fn metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }
    
    /// Validate an incoming order
    fn validate_order(&self, order: &Order) -> Result<(), MatcherError> {
        // Check if product is supported
        if !self.config.engine.product_ids.contains(&order.product_id) {
            return Err(MatcherError::ProductNotSupported { 
                product_id: order.product_id.clone() 
            });
        }
        
        // Check order quantity
        if order.quantity == 0 {
            return Err(MatcherError::InvalidOrder("Order quantity must be > 0".to_string()));
        }
        
        // Check order price for limit orders
        if matches!(order.order_type, crate::types::OrderType::Limit) && order.price == 0 {
            return Err(MatcherError::InvalidOrder("Limit order price must be > 0".to_string()));
        }
        
        // Check product-specific constraints
        if let Some(product_config) = self.config.get_product_config(&order.product_id) {
            // Check tick size
            if order.price % product_config.tick_size != 0 {
                return Err(MatcherError::InvalidOrder(
                    format!("Price {} is not a multiple of tick size {}", 
                           order.price, product_config.tick_size)
                ));
            }
            
            // Check lot size
            if order.quantity % product_config.lot_size != 0 {
                return Err(MatcherError::InvalidOrder(
                    format!("Quantity {} is not a multiple of lot size {}", 
                           order.quantity, product_config.lot_size)
                ));
            }
            
            // Check maximum order size
            if order.quantity > product_config.max_order_size {
                return Err(MatcherError::InvalidOrder(
                    format!("Order quantity {} exceeds maximum {}", 
                           order.quantity, product_config.max_order_size)
                ));
            }
        }
        
        Ok(())
    }
    
    /// Start background tasks for the engine
    pub async fn start_background_tasks(&self) -> Result<(), MatcherError> {
        // Start metrics collection
        let metrics = self.metrics.clone();
        
        // We need to periodically read the order books, so we'll access them in the task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // For now, just update with dummy values
                // In a real implementation, we'd need a different approach to access order books
                metrics.update_order_book_depth(0, 0);
            }
        });
        
        info!("Background tasks started");
        Ok(())
    }

    /// Replay WAL to restore state
    fn replay_wal(&self, wal: &WalManager) -> Result<(), MatcherError> {
        info!("Replaying WAL...");
        let entries = wal.replay()
            .map_err(|e| MatcherError::Engine(format!("Failed to replay WAL: {}", e)))?;
            
        let mut count = 0;
        for entry in entries {
            match entry {
                LogEntry::PlaceOrder(order) => {
                    // During replay, we ignore errors for individual orders to ensure
                    // we can restore as much state as possible.
                    if let Err(e) = self.process_order_memory(order) {
                        warn!("Error replaying PlaceOrder: {}", e);
                    }
                }
                LogEntry::CancelOrder(order_id, product_id) => {
                    if let Err(e) = self.process_cancel_memory(&product_id, order_id) {
                         warn!("Error replaying CancelOrder: {}", e);
                    }
                }
            }
            count += 1;
        }
        
        info!("WAL replay completed. Restored {} operations.", count);
        Ok(())
    }
}

/// High-level engine interface that wraps the matching engine
pub struct Engine {
    matching_engine: Arc<MatchingEngine>,
    config: Config,
}

impl Engine {
    /// Create a new engine instance
    pub async fn new(config: Config) -> Result<Self, MatcherError> {
        let matching_engine = Arc::new(MatchingEngine::new(config.clone()).await?);
        
        Ok(Self {
            matching_engine,
            config,
        })
    }
    
    /// Start the engine
    pub async fn start(&mut self) -> Result<(), MatcherError> {
        info!("Starting matching engine...");
        
        // Start background tasks
        self.matching_engine.start_background_tasks().await?;
        
        // Initialize tracing
        self.init_logging()?;
        
        info!("Matching engine started successfully");
        info!("Supported products: {:?}", self.config.engine.product_ids);
        info!("Listening on port: {}", self.config.network.listen_port);
        
        Ok(())
    }
    
    /// Get a reference to the matching engine
    pub fn matching_engine(&self) -> Arc<MatchingEngine> {
        self.matching_engine.clone()
    }
    
    /// Initialize logging based on configuration
    fn init_logging(&self) -> Result<(), MatcherError> {
        use tracing_subscriber::{EnvFilter, fmt, prelude::*};
        
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&self.config.monitoring.log_level));
        
        let fmt_layer = if self.config.monitoring.structured_logging {
            fmt::layer().with_target(false).boxed()
        } else {
            fmt::layer().boxed()
        };
        
        // Use try_init to avoid panic if already initialized
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init();
        
        Ok(())
    }
}