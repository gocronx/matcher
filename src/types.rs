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
#[non_exhaustive]
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
    pub fn market(id: OrderId, side: Side, quantity: Quantity) -> Self {
        Self::new(id, side, OrderType::Market, 0, quantity, 0)
    }

    pub fn limit(id: OrderId, side: Side, price: Price, quantity: Quantity) -> Self {
        Self::new(id, side, OrderType::Limit, price, quantity, 0)
    }

    pub fn ioc(id: OrderId, side: Side, price: Price, quantity: Quantity) -> Self {
        Self::new(id, side, OrderType::Ioc, price, quantity, 0)
    }

    pub fn fok(id: OrderId, side: Side, price: Price, quantity: Quantity) -> Self {
        Self::new(id, side, OrderType::Fok, price, quantity, 0)
    }

    pub fn post_only(id: OrderId, side: Side, price: Price, quantity: Quantity) -> Self {
        Self::new(id, side, OrderType::PostOnly, price, quantity, 0)
    }

    pub fn iceberg(
        id: OrderId,
        side: Side,
        price: Price,
        total_quantity: Quantity,
        visible: Quantity,
    ) -> Self {
        let visible = visible.min(total_quantity);
        let quantity = visible;
        Self::new(
            id,
            side,
            OrderType::Iceberg { visible },
            price,
            quantity,
            total_quantity.saturating_sub(quantity),
        )
    }

    fn new(
        id: OrderId,
        side: Side,
        kind: OrderType,
        price: Price,
        quantity: Quantity,
        hidden: Quantity,
    ) -> Self {
        Self {
            id,
            side,
            kind,
            price,
            quantity,
            filled: 0,
            hidden,
        }
    }

    pub fn remaining(&self) -> Quantity {
        self.quantity.saturating_sub(self.filled)
    }

    /// User-facing order size for a newly constructed order. For live book
    /// state, use `remaining() + hidden` because `filled` may be non-zero.
    pub fn total_quantity(&self) -> Quantity {
        self.quantity.saturating_add(self.hidden)
    }

    pub fn is_filled(&self) -> bool {
        self.filled >= self.quantity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_constructor_initializes_public_order_without_internal_state() {
        let order = Order::limit(42, Side::Buy, 100, 5);

        assert_eq!(order.id, 42);
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.kind, OrderType::Limit);
        assert_eq!(order.price, 100);
        assert_eq!(order.quantity, 5);
        assert_eq!(order.filled, 0);
        assert_eq!(order.hidden, 0);
    }

    #[test]
    fn market_constructor_uses_zero_price_and_requested_quantity() {
        let order = Order::market(7, Side::Sell, 3);

        assert_eq!(order.kind, OrderType::Market);
        assert_eq!(order.price, 0);
        assert_eq!(order.total_quantity(), 3);
    }

    #[test]
    fn iceberg_constructor_splits_total_into_visible_and_hidden() {
        let order = Order::iceberg(9, Side::Sell, 120, 25, 10);

        assert_eq!(order.kind, OrderType::Iceberg { visible: 10 });
        assert_eq!(order.quantity, 10);
        assert_eq!(order.hidden, 15);
        assert_eq!(order.total_quantity(), 25);
    }

    #[test]
    fn iceberg_constructor_caps_visible_size_to_total_quantity() {
        let order = Order::iceberg(9, Side::Sell, 120, 5, 10);

        assert_eq!(order.kind, OrderType::Iceberg { visible: 5 });
        assert_eq!(order.quantity, 5);
        assert_eq!(order.hidden, 0);
        assert_eq!(order.total_quantity(), 5);
    }
}

/// A successful match between two resting/aggressive orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trade {
    pub buy_id: OrderId,
    pub sell_id: OrderId,
    pub price: Price,
    pub quantity: Quantity,
    pub ts: Timestamp,
    /// Which side initiated the trade (the taker).
    pub aggressor: Side,
}

/// Why a submitted order was rejected before entering the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RejectReason {
    DuplicateOrderId,
    InvalidQuantity,
    InvalidPrice,
    PostOnlyWouldCross,
    FokNotFillable,
}

/// Why a cancel request could not be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CancelRejectReason {
    UnknownOrderId,
}

/// Public event stream emitted by the book for accepted, rejected, resting,
/// canceled, and traded orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BookEvent {
    /// A submit request passed validation and entered matching or resting logic.
    Accepted { order_id: OrderId },
    /// A successful match between the incoming order and a resting order.
    Trade(Trade),
    /// An order is now resting on the book with the given user-visible remaining
    /// quantity. This is emitted both when an order first rests and when a
    /// partially filled or replenished resting order has an updated remainder.
    Rested {
        order_id: OrderId,
        remaining: Quantity,
    },
    /// A cancel request removed an order from the book. There is no separate
    /// cancel-accepted event; this event is the successful cancel acknowledgement.
    Canceled {
        order_id: OrderId,
        remaining: Quantity,
    },
    /// A submit request was rejected before changing book state.
    Rejected {
        order_id: OrderId,
        reason: RejectReason,
    },
    /// A cancel request was rejected before changing book state.
    CancelRejected {
        order_id: OrderId,
        reason: CancelRejectReason,
    },
}
