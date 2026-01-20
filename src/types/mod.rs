use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;
use uuid::Uuid;

/// Product identifier type
pub type ProductId = String;

/// Order ID type using UUID for uniqueness
pub type OrderId = Uuid;

/// Price type (using u64 for fixed-point arithmetic)
pub type Price = u64;

/// Quantity type
pub type Quantity = u64;

/// Timestamp in nanoseconds
pub type Timestamp = u64;

/// Order side enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

/// Order type enumeration with advanced types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Market order - execute immediately at best available price
    Market,
    /// Limit order - execute only at specified price or better
    Limit,
    /// Immediate-or-Cancel - execute immediately, cancel remainder
    IOC,
    /// Fill-or-Kill - execute completely or cancel entirely
    FOK,
    /// Stop order - becomes market order when stop price is reached
    Stop { stop_price: Price },
    /// Stop-limit order - becomes limit order when stop price is reached
    StopLimit { stop_price: Price },
    /// Post-Only - only add liquidity, reject if would take
    PostOnly,
    /// Iceberg - hide total size, show only visible amount
    Iceberg { visible_size: Quantity },
}

/// Time-in-Force specification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeInForce {
    /// Good Till Cancel - remains active until explicitly cancelled
    GTC,
    /// Good Till Date - remains active until specified time
    GTD { expire_time: Timestamp },
    /// Day order - expires at end of trading day
    Day,
}

/// Order structure with enhanced features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub product_id: ProductId,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Price,
    pub quantity: Quantity,
    pub filled_quantity: Quantity,
    pub time_in_force: TimeInForce,
    pub submit_time: Timestamp,
    pub client_id: Option<String>,
    pub metadata: OrderMetadata,
    /// For Iceberg orders: remaining hidden quantity
    pub hidden_quantity: Quantity,
}

/// Additional order metadata
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderMetadata {
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub priority: u8,
}

impl Order {
    /// Create a new market order
    pub fn market(
        product_id: ProductId,
        side: Side,
        quantity: Quantity,
        submit_time: Timestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            side,
            order_type: OrderType::Market,
            price: 0,
            quantity,
            filled_quantity: 0,
            time_in_force: TimeInForce::GTC,
            submit_time,
            client_id: None,
            metadata: OrderMetadata::default(),
            hidden_quantity: 0,
        }
    }

    /// Create a new limit order
    pub fn limit(
        product_id: ProductId,
        side: Side,
        price: Price,
        quantity: Quantity,
        submit_time: Timestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            side,
            order_type: OrderType::Limit,
            price,
            quantity,
            filled_quantity: 0,
            time_in_force: TimeInForce::GTC,
            submit_time,
            client_id: None,
            metadata: OrderMetadata::default(),
            hidden_quantity: 0,
        }
    }

    /// Create a Post-Only order (only adds liquidity)
    pub fn post_only(
        product_id: ProductId,
        side: Side,
        price: Price,
        quantity: Quantity,
        submit_time: Timestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            side,
            order_type: OrderType::PostOnly,
            price,
            quantity,
            filled_quantity: 0,
            time_in_force: TimeInForce::GTC,
            submit_time,
            client_id: None,
            metadata: OrderMetadata::default(),
            hidden_quantity: 0,
        }
    }

    /// Create an Iceberg order (hides total size)
    pub fn iceberg(
        product_id: ProductId,
        side: Side,
        price: Price,
        total_quantity: Quantity,
        visible_quantity: Quantity,
        submit_time: Timestamp,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            side,
            order_type: OrderType::Iceberg { visible_size: visible_quantity },
            price,
            quantity: visible_quantity,  // Initially show only visible part
            filled_quantity: 0,
            time_in_force: TimeInForce::GTC,
            submit_time,
            client_id: None,
            metadata: OrderMetadata::default(),
            hidden_quantity: total_quantity - visible_quantity,
        }
    }

    /// Get remaining quantity
    pub fn remaining_quantity(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled_quantity)
    }

    /// Check if order is fully filled
    pub fn is_filled(&self) -> bool {
        self.filled_quantity >= self.quantity
    }

    /// Check if order can match with another order
    pub fn can_match(&self, other: &Order, current_time: Timestamp) -> bool {
        // Basic checks
        if self.product_id != other.product_id || self.side == other.side {
            return false;
        }

        // Time-in-force checks
        if !self.is_active(current_time) || !other.is_active(current_time) {
            return false;
        }

        // Price matching logic
        match (self.order_type, other.order_type) {
            (OrderType::Market, _) | (_, OrderType::Market) => true,
            (OrderType::Limit, OrderType::Limit) => {
                match self.side {
                    Side::Buy => self.price >= other.price,
                    Side::Sell => self.price <= other.price,
                }
            }
            _ => false, // More complex matching for other order types
        }
    }

    /// Check if order is still active based on time-in-force
    pub fn is_active(&self, current_time: Timestamp) -> bool {
        match self.time_in_force {
            TimeInForce::GTC => true,
            TimeInForce::GTD { expire_time } => current_time < expire_time,
            TimeInForce::Day => {
                // Simplified: assume day ends at midnight UTC
                // In practice, this would use market hours
                true
            }
        }
    }
}

/// Match result representing a successful trade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub trade_id: Uuid,
    pub product_id: ProductId,
    pub buy_order_id: OrderId,
    pub sell_order_id: OrderId,
    pub price: Price,
    pub quantity: Quantity,
    pub trade_time: Timestamp,
    pub match_latency_ns: u64,
    pub aggressor_side: Side,
}

/// Order book level (price and quantity)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Price,
    pub quantity: Quantity,
    pub order_count: u32,
}

/// Order book snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookSnapshot {
    pub product_id: ProductId,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub timestamp: Timestamp,
}

/// Engine statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStats {
    pub orders_received: u64,
    pub orders_matched: u64,
    pub trades_executed: u64,
    pub avg_match_latency_ns: u64,
    pub uptime_seconds: u64,
}

/// Error types for the matching engine
#[derive(Error, Debug)]
pub enum MatcherError {
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Network error: {0}")]
    Network(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Invalid order: {0}")]
    InvalidOrder(String),
    
    #[error("Order not found: {order_id}")]
    OrderNotFound { order_id: OrderId },
    
    #[error("Product not supported: {product_id}")]
    ProductNotSupported { product_id: ProductId },
    
    #[error("Engine error: {0}")]
    Engine(String),
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_creation() {
        let order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, 1000);
        assert_eq!(order.product_id, "BTC-USD");
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.price, 50000);
        assert_eq!(order.quantity, 100);
        assert_eq!(order.filled_quantity, 0);
    }

    #[test]
    fn test_order_remaining_quantity() {
        let mut order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, 1000);
        assert_eq!(order.remaining_quantity(), 100);
        
        order.filled_quantity = 30;
        assert_eq!(order.remaining_quantity(), 70);
        
        order.filled_quantity = 100;
        assert_eq!(order.remaining_quantity(), 0);
    }

    #[test]
    fn test_order_is_filled() {
        let mut order = Order::limit("BTC-USD".to_string(), Side::Buy, 50000, 100, 1000);
        assert!(!order.is_filled());
        
        order.filled_quantity = 100;
        assert!(order.is_filled());
    }

    #[test]
    fn test_side_display() {
        assert_eq!(format!("{}", Side::Buy), "BUY");
        assert_eq!(format!("{}", Side::Sell), "SELL");
    }

    #[test]
    fn test_market_order_creation() {
        let order = Order::market("ETH-USD".to_string(), Side::Sell, 50, 2000);
        assert_eq!(order.price, 0);
        assert!(matches!(order.order_type, OrderType::Market));
    }
}