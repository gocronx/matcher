use prometheus::{
    Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Opts, Registry,
};
use std::sync::Arc;
use crate::types::MatcherError;

/// Metrics collector for the matching engine
/// 
/// Provides comprehensive metrics collection including:
/// - Order processing rates
/// - Matching latency histograms  
/// - Order book depth
/// - Error rates
/// - System resource usage
#[derive(Clone)]
pub struct Metrics {
    registry: Arc<Registry>,
    
    // Order metrics
    orders_received: IntCounter,
    orders_matched: IntCounter,
    orders_cancelled: IntCounter,
    orders_rejected: IntCounter,
    
    // Latency metrics
    match_latency: Histogram,
    network_latency: Histogram,
    
    // Order book metrics
    order_book_depth_bids: IntGauge,
    order_book_depth_asks: IntGauge,
    
    // System metrics
    memory_usage: Gauge,
    cpu_usage: Gauge,
    
    // Error metrics
    errors_total: IntCounter,
}

impl Metrics {
    /// Create a new metrics collector
    pub fn new() -> Result<Self, MatcherError> {
        let registry = Arc::new(Registry::new());
        
        // Order metrics
        let orders_received = IntCounter::with_opts(
            Opts::new("orders_received_total", "Total number of orders received")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create orders_received metric: {}", e)))?;
        
        let orders_matched = IntCounter::with_opts(
            Opts::new("orders_matched_total", "Total number of orders matched")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create orders_matched metric: {}", e)))?;
        
        let orders_cancelled = IntCounter::with_opts(
            Opts::new("orders_cancelled_total", "Total number of orders cancelled")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create orders_cancelled metric: {}", e)))?;
        
        let orders_rejected = IntCounter::with_opts(
            Opts::new("orders_rejected_total", "Total number of orders rejected")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create orders_rejected metric: {}", e)))?;
        
        // Latency metrics with buckets optimized for trading
        let match_latency = Histogram::with_opts(
            HistogramOpts::new("match_latency_seconds", "Order matching latency")
                .buckets(vec![
                    0.000_000_1,  // 100ns
                    0.000_000_5,  // 500ns
                    0.000_001,    // 1μs
                    0.000_005,    // 5μs
                    0.000_01,     // 10μs
                    0.000_05,     // 50μs
                    0.000_1,      // 100μs
                    0.000_5,      // 500μs
                    0.001,        // 1ms
                    0.005,        // 5ms
                    0.01,         // 10ms
                ])
        ).map_err(|e| MatcherError::Engine(format!("Failed to create match_latency metric: {}", e)))?;
        
        let network_latency = Histogram::with_opts(
            HistogramOpts::new("network_latency_seconds", "Network processing latency")
                .buckets(vec![
                    0.000_001,    // 1μs
                    0.000_01,     // 10μs
                    0.000_1,      // 100μs
                    0.001,        // 1ms
                    0.01,         // 10ms
                    0.1,          // 100ms
                ])
        ).map_err(|e| MatcherError::Engine(format!("Failed to create network_latency metric: {}", e)))?;
        
        // Order book metrics
        let order_book_depth_bids = IntGauge::with_opts(
            Opts::new("order_book_depth_bids", "Number of bid orders in the book")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create order_book_depth_bids metric: {}", e)))?;
        
        let order_book_depth_asks = IntGauge::with_opts(
            Opts::new("order_book_depth_asks", "Number of ask orders in the book")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create order_book_depth_asks metric: {}", e)))?;
        
        // System metrics
        let memory_usage = Gauge::with_opts(
            Opts::new("memory_usage_bytes", "Memory usage in bytes")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create memory_usage metric: {}", e)))?;
        
        let cpu_usage = Gauge::with_opts(
            Opts::new("cpu_usage_percent", "CPU usage percentage")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create cpu_usage metric: {}", e)))?;
        
        // Error metrics
        let errors_total = IntCounter::with_opts(
            Opts::new("errors_total", "Total number of errors")
        ).map_err(|e| MatcherError::Engine(format!("Failed to create errors_total metric: {}", e)))?;
        
        // Register all metrics
        registry.register(Box::new(orders_received.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register orders_received: {}", e)))?;
        registry.register(Box::new(orders_matched.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register orders_matched: {}", e)))?;
        registry.register(Box::new(orders_cancelled.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register orders_cancelled: {}", e)))?;
        registry.register(Box::new(orders_rejected.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register orders_rejected: {}", e)))?;
        registry.register(Box::new(match_latency.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register match_latency: {}", e)))?;
        registry.register(Box::new(network_latency.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register network_latency: {}", e)))?;
        registry.register(Box::new(order_book_depth_bids.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register order_book_depth_bids: {}", e)))?;
        registry.register(Box::new(order_book_depth_asks.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register order_book_depth_asks: {}", e)))?;
        registry.register(Box::new(memory_usage.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register memory_usage: {}", e)))?;
        registry.register(Box::new(cpu_usage.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register cpu_usage: {}", e)))?;
        registry.register(Box::new(errors_total.clone()))
            .map_err(|e| MatcherError::Engine(format!("Failed to register errors_total: {}", e)))?;
        
        Ok(Self {
            registry,
            orders_received,
            orders_matched,
            orders_cancelled,
            orders_rejected,
            match_latency,
            network_latency,
            order_book_depth_bids,
            order_book_depth_asks,
            memory_usage,
            cpu_usage,
            errors_total,
        })
    }
    
    /// Record an order received
    pub fn record_order_received(&self) {
        self.orders_received.inc();
    }
    
    /// Record an order matched
    pub fn record_order_matched(&self) {
        self.orders_matched.inc();
    }
    
    /// Record an order cancelled
    pub fn record_order_cancelled(&self) {
        self.orders_cancelled.inc();
    }
    
    /// Record an order rejected
    pub fn record_order_rejected(&self) {
        self.orders_rejected.inc();
    }
    
    /// Record matching latency in nanoseconds
    pub fn record_match_latency(&self, latency_ns: u64) {
        let latency_seconds = latency_ns as f64 / 1_000_000_000.0;
        self.match_latency.observe(latency_seconds);
    }
    
    /// Record network latency in nanoseconds
    pub fn record_network_latency(&self, latency_ns: u64) {
        let latency_seconds = latency_ns as f64 / 1_000_000_000.0;
        self.network_latency.observe(latency_seconds);
    }
    
    /// Update order book depth
    pub fn update_order_book_depth(&self, bids: i64, asks: i64) {
        self.order_book_depth_bids.set(bids);
        self.order_book_depth_asks.set(asks);
    }
    
    /// Update memory usage in bytes
    pub fn update_memory_usage(&self, bytes: f64) {
        self.memory_usage.set(bytes);
    }
    
    /// Update CPU usage percentage
    pub fn update_cpu_usage(&self, percent: f64) {
        self.cpu_usage.set(percent);
    }
    
    /// Record an error
    pub fn record_error(&self) {
        self.errors_total.inc();
    }
    
    /// Get the Prometheus registry for HTTP exposition
    pub fn registry(&self) -> Arc<Registry> {
        self.registry.clone()
    }
    
    /// Get metrics as Prometheus text format
    pub fn gather(&self) -> String {
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        encoder.encode_to_string(&metric_families).unwrap_or_default()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create default metrics")
    }
}

/// System resource monitor
pub struct ResourceMonitor {
    metrics: Metrics,
}

impl ResourceMonitor {
    pub fn new(metrics: Metrics) -> Self {
        Self { metrics }
    }
    
    /// Start monitoring system resources in the background
    pub async fn start_monitoring(&self) {
        let metrics = self.metrics.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                
                // Update memory usage (simplified - in production use a proper system monitoring crate)
                if let Ok(memory) = Self::get_memory_usage() {
                    metrics.update_memory_usage(memory as f64);
                }
                
                // Update CPU usage (simplified)
                if let Ok(cpu) = Self::get_cpu_usage() {
                    metrics.update_cpu_usage(cpu);
                }
            }
        });
    }
    
    fn get_memory_usage() -> Result<u64, std::io::Error> {
        // Simplified memory usage - in production, use a proper system monitoring library
        // This is just a placeholder
        Ok(1024 * 1024 * 100) // 100MB placeholder
    }
    
    fn get_cpu_usage() -> Result<f64, std::io::Error> {
        // Simplified CPU usage - in production, use a proper system monitoring library
        // This is just a placeholder
        Ok(25.0) // 25% placeholder
    }
}