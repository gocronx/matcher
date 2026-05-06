//! Core domain types: orders, trades, sides.
//!
//! No serde, no UUIDs, no chrono — everything is a plain integer or enum so
//! the wire codec can encode each field with one `to_be_bytes` call.

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};

// ---------------------------------------------------------------------------
// Newtypes
// ---------------------------------------------------------------------------

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct OrderId(pub u64);

impl OrderId {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for OrderId {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<OrderId> for u64 {
    fn from(v: OrderId) -> Self {
        v.0
    }
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

// ---------------------------------------------------------------------------

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Price(pub u64);

impl Price {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for Price {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<Price> for u64 {
    fn from(v: Price) -> Self {
        v.0
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

// ---------------------------------------------------------------------------

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Quantity(pub u64);

impl Quantity {
    pub const ZERO: Quantity = Quantity(0);

    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
    pub const fn saturating_add(self, rhs: Quantity) -> Quantity {
        Quantity(self.0.saturating_add(rhs.0))
    }
    pub const fn saturating_sub(self, rhs: Quantity) -> Quantity {
        Quantity(self.0.saturating_sub(rhs.0))
    }
    pub fn min(self, rhs: Quantity) -> Quantity {
        if self.0 <= rhs.0 {
            self
        } else {
            rhs
        }
    }
}

impl From<u64> for Quantity {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<Quantity> for u64 {
    fn from(v: Quantity) -> Self {
        v.0
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Add for Quantity {
    type Output = Quantity;
    fn add(self, rhs: Quantity) -> Quantity {
        // Use saturating arithmetic to prevent silent overflow in financial calculations
        Quantity(self.0.saturating_add(rhs.0))
    }
}

impl AddAssign for Quantity {
    fn add_assign(&mut self, rhs: Quantity) {
        self.0 = self.0.saturating_add(rhs.0);
    }
}

impl Sub for Quantity {
    type Output = Quantity;
    fn sub(self, rhs: Quantity) -> Quantity {
        // Use saturating arithmetic to prevent underflow
        Quantity(self.0.saturating_sub(rhs.0))
    }
}

impl SubAssign for Quantity {
    fn sub_assign(&mut self, rhs: Quantity) {
        self.0 = self.0.saturating_sub(rhs.0);
    }
}

impl Quantity {
    /// Checked addition. Returns None if overflow would occur.
    pub const fn checked_add(self, rhs: Quantity) -> Option<Quantity> {
        match self.0.checked_add(rhs.0) {
            Some(v) => Some(Quantity(v)),
            None => None,
        }
    }

    /// Checked subtraction. Returns None if underflow would occur.
    pub const fn checked_sub(self, rhs: Quantity) -> Option<Quantity> {
        match self.0.checked_sub(rhs.0) {
            Some(v) => Some(Quantity(v)),
            None => None,
        }
    }
}

// ---------------------------------------------------------------------------

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Timestamp(pub u64);

impl Timestamp {
    pub const fn new(v: u64) -> Self {
        Self(v)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for Timestamp {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<Timestamp> for u64 {
    fn from(v: Timestamp) -> Self {
        v.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

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
    /// Hidden remainder for Iceberg orders, Quantity::ZERO otherwise.
    pub hidden: Quantity,
}

impl Order {
    pub fn market(id: impl Into<OrderId>, side: Side, quantity: impl Into<Quantity>) -> Self {
        Self::new(
            id.into(),
            side,
            OrderType::Market,
            Price::ZERO,
            quantity.into(),
            Quantity::ZERO,
        )
    }

    pub fn limit(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        quantity: impl Into<Quantity>,
    ) -> Self {
        Self::new(
            id.into(),
            side,
            OrderType::Limit,
            price.into(),
            quantity.into(),
            Quantity::ZERO,
        )
    }

    pub fn ioc(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        quantity: impl Into<Quantity>,
    ) -> Self {
        Self::new(
            id.into(),
            side,
            OrderType::Ioc,
            price.into(),
            quantity.into(),
            Quantity::ZERO,
        )
    }

    pub fn fok(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        quantity: impl Into<Quantity>,
    ) -> Self {
        Self::new(
            id.into(),
            side,
            OrderType::Fok,
            price.into(),
            quantity.into(),
            Quantity::ZERO,
        )
    }

    pub fn post_only(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        quantity: impl Into<Quantity>,
    ) -> Self {
        Self::new(
            id.into(),
            side,
            OrderType::PostOnly,
            price.into(),
            quantity.into(),
            Quantity::ZERO,
        )
    }

    pub fn iceberg(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        total_quantity: impl Into<Quantity>,
        visible: impl Into<Quantity>,
    ) -> Self {
        let total_quantity = total_quantity.into();
        let visible = visible.into().min(total_quantity);
        let quantity = visible;
        Self::new(
            id.into(),
            side,
            OrderType::Iceberg { visible },
            price.into(),
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
            filled: Quantity::ZERO,
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

// Add a ZERO constant on Price for internal use.
impl Price {
    pub const ZERO: Price = Price(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_constructor_initializes_public_order_without_internal_state() {
        let order = Order::limit(42, Side::Buy, 100, 5);

        assert_eq!(order.id, OrderId(42));
        assert_eq!(order.side, Side::Buy);
        assert_eq!(order.kind, OrderType::Limit);
        assert_eq!(order.price, Price(100));
        assert_eq!(order.quantity, Quantity(5));
        assert_eq!(order.filled, Quantity::ZERO);
        assert_eq!(order.hidden, Quantity::ZERO);
    }

    #[test]
    fn market_constructor_uses_zero_price_and_requested_quantity() {
        let order = Order::market(7, Side::Sell, 3);

        assert_eq!(order.kind, OrderType::Market);
        assert_eq!(order.price, Price::ZERO);
        assert_eq!(order.total_quantity(), Quantity(3));
    }

    #[test]
    fn iceberg_constructor_splits_total_into_visible_and_hidden() {
        let order = Order::iceberg(9, Side::Sell, 120, 25, 10);

        assert_eq!(
            order.kind,
            OrderType::Iceberg {
                visible: Quantity(10)
            }
        );
        assert_eq!(order.quantity, Quantity(10));
        assert_eq!(order.hidden, Quantity(15));
        assert_eq!(order.total_quantity(), Quantity(25));
    }

    #[test]
    fn iceberg_constructor_caps_visible_size_to_total_quantity() {
        let order = Order::iceberg(9, Side::Sell, 120, 5, 10);

        assert_eq!(
            order.kind,
            OrderType::Iceberg {
                visible: Quantity(5)
            }
        );
        assert_eq!(order.quantity, Quantity(5));
        assert_eq!(order.hidden, Quantity::ZERO);
        assert_eq!(order.total_quantity(), Quantity(5));
    }

    #[test]
    fn quantity_add_saturates_instead_of_wrapping() {
        let max = Quantity(u64::MAX);
        let one = Quantity(1);
        
        // Should saturate at MAX, not wrap to 0
        assert_eq!(max + one, Quantity(u64::MAX));
        assert_eq!(max.saturating_add(one), Quantity(u64::MAX));
    }

    #[test]
    fn quantity_sub_saturates_instead_of_wrapping() {
        let zero = Quantity(0);
        let one = Quantity(1);
        
        // Should saturate at 0, not wrap to MAX
        assert_eq!(zero - one, Quantity(0));
        assert_eq!(zero.saturating_sub(one), Quantity(0));
    }

    #[test]
    fn quantity_checked_add_detects_overflow() {
        let max = Quantity(u64::MAX);
        let one = Quantity(1);
        
        assert_eq!(max.checked_add(one), None);
        assert_eq!(Quantity(100).checked_add(Quantity(50)), Some(Quantity(150)));
    }

    #[test]
    fn quantity_checked_sub_detects_underflow() {
        let zero = Quantity(0);
        let one = Quantity(1);
        
        assert_eq!(zero.checked_sub(one), None);
        assert_eq!(Quantity(100).checked_sub(Quantity(50)), Some(Quantity(50)));
    }

    #[test]
    fn quantity_add_assign_saturates() {
        let mut qty = Quantity(u64::MAX - 10);
        qty += Quantity(20);
        assert_eq!(qty, Quantity(u64::MAX));
    }

    #[test]
    fn quantity_sub_assign_saturates() {
        let mut qty = Quantity(5);
        qty -= Quantity(10);
        assert_eq!(qty, Quantity(0));
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

/// Why an order amendment was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AmendRejectReason {
    UnknownOrderId,
    InvalidPrice,
    InvalidQuantity,
    /// Cannot increase quantity on an existing order (only decrease allowed)
    QuantityIncrease,
    /// Cannot amend market orders or other non-resting order types
    OrderTypeNotAmendable,
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
        ts: Timestamp,
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
    /// An order amendment was successfully applied.
    Amended {
        order_id: OrderId,
        new_price: Option<Price>,
        new_quantity: Quantity,
    },
    /// An amendment request was rejected.
    AmendRejected {
        order_id: OrderId,
        reason: AmendRejectReason,
    },
}
