//! Core domain types: orders, trades, sides.
//!
//! No serde, no UUIDs, no chrono — everything is a plain integer or enum so
//! the wire codec can encode each field with one `to_be_bytes` call.

pub type OrderId = u64;
pub type Price = u64;
pub type Quantity = u64;
pub type Timestamp = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    /// Take whatever liquidity is available, ignore price.
    Market,
    /// Rest on the book at `price` if it does not fully match.
    Limit,
    /// Match what you can immediately, cancel the rest.
    Ioc,
    /// All-or-nothing — match completely or reject.
    Fok,
    /// Reject if the order would take liquidity (maker-only).
    PostOnly,
    /// Limit order whose visible size is `visible`; the rest is hidden and
    /// replenished as the visible portion fills.
    Iceberg { visible: Quantity },
}

/// A new order arriving at the engine.
#[derive(Debug, Clone)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub kind: OrderType,
    pub price: Price,
    pub quantity: Quantity,
    pub filled: Quantity,
    /// Hidden remainder for Iceberg orders, 0 otherwise.
    pub hidden: Quantity,
}

impl Order {
    pub fn remaining(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled)
    }

    pub fn is_filled(&self) -> bool {
        self.filled >= self.quantity
    }
}

/// A successful match between two resting/aggressive orders.
#[derive(Debug, Clone, Copy)]
pub struct Trade {
    pub buy_id: OrderId,
    pub sell_id: OrderId,
    pub price: Price,
    pub quantity: Quantity,
    pub ts: Timestamp,
    /// Which side initiated the trade (the taker).
    pub aggressor: Side,
}
