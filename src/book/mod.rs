pub mod amend;
pub mod matching;
pub mod snapshot;

pub use snapshot::SnapshotError;

use crate::types::{
    BookEvent, CancelRejectReason, Order, OrderId, OrderType, Price, Quantity, RejectReason, Side,
    Timestamp, Trade,
};
use ahash::AHashMap;
use smallvec::SmallVec;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Aggregates the per-match context passed to `fill_one_resting`.
pub(super) struct MatchContext {
    pub(super) price: Price,
    pub(super) now: Timestamp,
    pub(super) aggressor: Side,
}

/// Configuration for `OrderBook::check_side_invariants` (test/fuzzing only).
#[cfg(any(test, feature = "fuzzing"))]
pub(super) struct SideConfig {
    pub(super) expected_side: Side,
    /// true for asks (ascending iteration → first = best); false for bids (last = best).
    pub(super) first_is_best: bool,
}

/// Snapshot of a resting order's fields captured before it is mutated/removed.
/// Avoids borrowing `self.orders` while also mutating the book.
pub(super) struct RestingSnap {
    pub(super) id: OrderId,
    pub(super) side: Side,
    pub(super) kind: OrderType,
    pub(super) price: Price,
    pub(super) hidden: Quantity,
    pub(super) filled: bool,
}

impl RestingSnap {
    pub(super) fn from_order(o: &Order, id: OrderId) -> Self {
        Self {
            id,
            side: o.side,
            kind: o.kind,
            price: o.price,
            hidden: o.hidden,
            filled: o.is_filled(),
        }
    }
}

pub(super) struct PriceLevel {
    pub(super) orders: SmallVec<[OrderId; 8]>,
    pub(super) total_qty: Quantity,
}

impl PriceLevel {
    pub(super) fn new() -> Self {
        Self {
            orders: SmallVec::new(),
            total_qty: Quantity::ZERO,
        }
    }
    pub(super) fn add(&mut self, id: OrderId, qty: Quantity) {
        self.orders.push(id);
        self.total_qty += qty;
    }
    pub(super) fn remove(&mut self, id: OrderId, qty: Quantity) {
        if let Some(p) = self.orders.iter().position(|&x| x == id) {
            self.orders.remove(p);
            self.total_qty = self.total_qty.saturating_sub(qty);
        }
    }
}

// ---------------------------------------------------------------------------
// OrderBook
// ---------------------------------------------------------------------------

pub struct OrderBook {
    pub(super) orders: AHashMap<OrderId, Order>,
    pub(super) bids: BTreeMap<Price, PriceLevel>,
    pub(super) asks: BTreeMap<Price, PriceLevel>,
    pub(super) best_bid: Option<Price>,
    pub(super) best_ask: Option<Price>,
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            orders: AHashMap::with_capacity(1024),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            best_bid: None,
            best_ask: None,
        }
    }

    pub fn best_bid(&self) -> Option<Price> {
        self.best_bid
    }
    pub fn best_ask(&self) -> Option<Price> {
        self.best_ask
    }
    pub fn len(&self) -> usize {
        self.orders.len()
    }
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Top N bid levels (price descending) as (price, visible quantity).
    /// Returns fewer entries than N if the book is shallower.
    pub fn top_bids(&self, n: usize) -> Vec<(Price, Quantity)> {
        self.bids
            .iter()
            .rev()
            .take(n)
            .map(|(&p, l)| (p, l.total_qty))
            .collect()
    }

    /// Top N ask levels (price ascending) as (price, visible quantity).
    pub fn top_asks(&self, n: usize) -> Vec<(Price, Quantity)> {
        self.asks
            .iter()
            .take(n)
            .map(|(&p, l)| (p, l.total_qty))
            .collect()
    }

    /// Total visible quantity at a specific price level on the given side.
    /// Returns Quantity::ZERO if no level exists at that price.
    pub fn level_qty(&self, side: Side, price: impl Into<Price>) -> Quantity {
        let price = price.into();
        match side {
            Side::Buy => self
                .bids
                .get(&price)
                .map_or(Quantity::ZERO, |l| l.total_qty),
            Side::Sell => self
                .asks
                .get(&price)
                .map_or(Quantity::ZERO, |l| l.total_qty),
        }
    }

    #[cfg(any(test, feature = "fuzzing"))]
    pub(super) fn assert_invariants(&self) {
        self.assert_invariants_at("book", 0, 0);
    }

    #[cfg(any(test, feature = "fuzzing"))]
    pub(super) fn assert_invariants_at(&self, scenario: &str, seed: u64, step: usize) {
        let mut seen = std::collections::HashSet::new();
        let ctx = || format!("{scenario} seed={seed} step={step}");

        let expected_bid = self.check_side_invariants(
            self.bids.iter(),
            SideConfig {
                expected_side: Side::Buy,
                first_is_best: false,
            },
            &mut seen,
            &ctx,
        );
        let expected_ask = self.check_side_invariants(
            self.asks.iter(),
            SideConfig {
                expected_side: Side::Sell,
                first_is_best: true,
            },
            &mut seen,
            &ctx,
        );

        assert_eq!(self.best_bid, expected_bid, "{}", ctx());
        assert_eq!(self.best_ask, expected_ask, "{}", ctx());
        assert_eq!(self.orders.len(), seen.len(), "{}", ctx());
        if let (Some(bid), Some(ask)) = (self.best_bid, self.best_ask) {
            assert!(bid < ask, "{}: crossed book: bid={bid} ask={ask}", ctx());
        }
    }

    /// Validate every price level on one side; return the expected best price.
    #[cfg(any(test, feature = "fuzzing"))]
    fn check_side_invariants<'a>(
        &self,
        levels: impl Iterator<Item = (&'a Price, &'a PriceLevel)>,
        cfg: SideConfig,
        seen: &mut std::collections::HashSet<OrderId>,
        ctx: &impl Fn() -> String,
    ) -> Option<Price> {
        let mut best: Option<Price> = None;
        for (&price, level) in levels {
            assert!(
                !level.orders.is_empty(),
                "{}: empty level at {price}",
                ctx()
            );
            if cfg.first_is_best && best.is_none() {
                best = Some(price);
            } else if !cfg.first_is_best {
                best = Some(price); // last wins for bids
            }
            let mut total = Quantity::ZERO;
            for id in &level.orders {
                assert!(
                    seen.insert(*id),
                    "{}: order {id} appears in multiple levels",
                    ctx()
                );
                let order = self
                    .orders
                    .get(id)
                    .unwrap_or_else(|| panic!("{}: level references missing order", ctx()));
                assert_eq!(order.side, cfg.expected_side, "{}", ctx());
                assert_eq!(order.price, price, "{}", ctx());
                total += Self::order_level_qty(order);
            }
            assert_eq!(level.total_qty, total, "{}: bad total at {price}", ctx());
        }
        best
    }

    pub(super) fn iceberg_visible(kind: OrderType) -> Option<Quantity> {
        if let OrderType::Iceberg { visible } = kind {
            Some(visible)
        } else {
            None
        }
    }

    pub(super) fn order_level_qty(o: &Order) -> Quantity {
        Self::iceberg_visible(o.kind).map_or(o.remaining(), |v| v.min(o.remaining()))
    }

    pub(super) fn user_remaining(o: &Order) -> Quantity {
        o.remaining().saturating_add(o.hidden)
    }

    fn validate_order(&self, order: &Order) -> Option<RejectReason> {
        if self.orders.contains_key(&order.id) {
            return Some(RejectReason::DuplicateOrderId);
        }
        if order.quantity == Quantity::ZERO {
            return Some(RejectReason::InvalidQuantity);
        }
        if matches!(order.kind, OrderType::Iceberg { visible } if visible == Quantity::ZERO) {
            return Some(RejectReason::InvalidQuantity);
        }
        if !matches!(order.kind, OrderType::Market) && order.price == Price::ZERO {
            return Some(RejectReason::InvalidPrice);
        }
        None
    }

    pub(super) fn rest(&mut self, order: Order) {
        let (id, price, side, qty) = (
            order.id,
            order.price,
            order.side,
            Self::order_level_qty(&order),
        );
        self.orders.insert(id, order);
        match side {
            Side::Buy => {
                self.bids
                    .entry(price)
                    .or_insert_with(PriceLevel::new)
                    .add(id, qty);
                if self.best_bid.is_none_or(|b| price > b) {
                    self.best_bid = Some(price);
                }
            }
            Side::Sell => {
                self.asks
                    .entry(price)
                    .or_insert_with(PriceLevel::new)
                    .add(id, qty);
                if self.best_ask.is_none_or(|a| price < a) {
                    self.best_ask = Some(price);
                }
            }
        }
    }

    pub(super) fn drop_level(&mut self, side: Side, price: Price) {
        match side {
            Side::Buy => {
                self.bids.remove(&price);
                if self.best_bid == Some(price) {
                    self.best_bid = self.bids.keys().next_back().copied();
                }
            }
            Side::Sell => {
                self.asks.remove(&price);
                if self.best_ask == Some(price) {
                    self.best_ask = self.asks.keys().next().copied();
                }
            }
        }
    }

    fn remove_resting_order(&mut self, id: OrderId) -> Option<Order> {
        let o = self.orders.remove(&id)?;
        let (price, qty) = (o.price, Self::order_level_qty(&o));
        let levels = match o.side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        if let Some(lvl) = levels.get_mut(&price) {
            lvl.remove(id, qty);
            if lvl.orders.is_empty() {
                self.drop_level(o.side, price);
            }
        }
        Some(o)
    }

    pub fn cancel(&mut self, id: impl Into<OrderId>) -> bool {
        self.remove_resting_order(id.into()).is_some()
    }

    pub fn cancel_events(
        &mut self,
        id: impl Into<OrderId>,
        ts: impl Into<Timestamp>,
    ) -> Vec<BookEvent> {
        let id = id.into();
        let ts = ts.into();
        match self.remove_resting_order(id) {
            Some(order) => vec![BookEvent::Canceled {
                order_id: id,
                remaining: Self::user_remaining(&order),
                ts,
            }],
            None => vec![BookEvent::CancelRejected {
                order_id: id,
                reason: CancelRejectReason::UnknownOrderId,
            }],
        }
    }

    pub(super) fn can_fill(&self, order: &Order) -> bool {
        let mut avail = Quantity::ZERO;
        let need = order.remaining();
        match order.side {
            Side::Buy => {
                for (&p, l) in &self.asks {
                    if p > order.price {
                        break;
                    }
                    avail += l.total_qty;
                    if avail >= need {
                        return true;
                    }
                }
            }
            Side::Sell => {
                for (&p, l) in self.bids.iter().rev() {
                    if p < order.price {
                        break;
                    }
                    avail += l.total_qty;
                    if avail >= need {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn price_list(&self, incoming: &Order) -> Vec<Price> {
        match incoming.side {
            Side::Buy => {
                if matches!(incoming.kind, OrderType::Market) {
                    self.asks.keys().copied().collect()
                } else {
                    self.asks
                        .range(..=incoming.price)
                        .map(|(&p, _)| p)
                        .collect()
                }
            }
            Side::Sell => {
                if matches!(incoming.kind, OrderType::Market) {
                    self.bids.keys().rev().copied().collect()
                } else {
                    self.bids
                        .range(incoming.price..)
                        .rev()
                        .map(|(&p, _)| p)
                        .collect()
                }
            }
        }
    }

    pub fn submit(&mut self, order: Order, now: impl Into<Timestamp>) -> Vec<Trade> {
        self.submit_events(order, now.into())
            .into_iter()
            .filter_map(|event| match event {
                BookEvent::Trade(trade) => Some(trade),
                _ => None,
            })
            .collect()
    }

    pub fn submit_events(&mut self, mut order: Order, now: impl Into<Timestamp>) -> Vec<BookEvent> {
        let now = now.into();
        let mut events = Vec::new();

        if let Some(reason) = self.validate_order(&order) {
            return vec![BookEvent::Rejected {
                order_id: order.id,
                reason,
            }];
        }

        if matches!(order.kind, OrderType::PostOnly) {
            return self.handle_post_only(order, &mut events);
        }

        if matches!(order.kind, OrderType::Fok) && !self.can_fill(&order) {
            return vec![BookEvent::Rejected {
                order_id: order.id,
                reason: RejectReason::FokNotFillable,
            }];
        }

        events.push(BookEvent::Accepted { order_id: order.id });
        self.match_side(&mut order, now, &mut events);
        self.rest_unfilled_remainder(order, &mut events);
        events
    }
}

// ---------------------------------------------------------------------------
// Tests — cancel / amend / depth query
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BookEvent, CancelRejectReason, Order, OrderId, Price, Quantity, Side, Timestamp,
    };

    fn lim(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        qty: impl Into<Quantity>,
    ) -> Order {
        Order::limit(id, side, price, qty)
    }

    #[test]
    fn cancel_events_reports_success_and_unknown_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        let events = b.cancel_events(1u64, 123);
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Canceled {
                order_id,
                remaining,
                ts,
            } => {
                assert_eq!(order_id, OrderId(1));
                assert_eq!(remaining, Quantity(10));
                assert_eq!(ts, Timestamp(123));
            }
            _ => panic!("expected Canceled event"),
        }

        assert_eq!(
            b.cancel_events(1u64, 124),
            vec![BookEvent::CancelRejected {
                order_id: OrderId(1),
                reason: CancelRejectReason::UnknownOrderId,
            }]
        );
    }

    #[test]
    fn top_bids_returns_levels_in_descending_price_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.submit(lim(2, Side::Buy, 102, 3), 0);
        b.submit(lim(3, Side::Buy, 101, 7), 0);
        b.assert_invariants();

        let levels = b.top_bids(3);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], (Price(102), Quantity(3)));
        assert_eq!(levels[1], (Price(101), Quantity(7)));
        assert_eq!(levels[2], (Price(100), Quantity(5)));
    }

    #[test]
    fn top_asks_returns_levels_in_ascending_price_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 103, 4), 0);
        b.submit(lim(2, Side::Sell, 101, 6), 0);
        b.submit(lim(3, Side::Sell, 102, 2), 0);
        b.assert_invariants();

        let levels = b.top_asks(3);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[0], (Price(101), Quantity(6)));
        assert_eq!(levels[1], (Price(102), Quantity(2)));
        assert_eq!(levels[2], (Price(103), Quantity(4)));
    }

    #[test]
    fn top_n_caps_at_book_depth_when_n_exceeds() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.submit(lim(2, Side::Buy, 101, 3), 0);
        b.assert_invariants();

        let levels = b.top_bids(10);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], (Price(101), Quantity(3)));
        assert_eq!(levels[1], (Price(100), Quantity(5)));
    }

    #[test]
    fn top_n_returns_empty_when_side_is_empty() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.assert_invariants();

        let ask_levels = b.top_asks(5);
        assert!(ask_levels.is_empty());

        let bid_levels = b.top_bids(0);
        assert!(bid_levels.is_empty());
    }

    #[test]
    fn level_qty_aggregates_orders_at_same_price() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 100, 8), 0);
        b.submit(lim(3, Side::Sell, 101, 3), 0);
        b.assert_invariants();

        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(13));
        assert_eq!(b.level_qty(Side::Sell, 101u64), Quantity(3));
    }

    #[test]
    fn level_qty_returns_zero_for_unknown_price() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.assert_invariants();

        assert_eq!(b.level_qty(Side::Buy, 99u64), Quantity::ZERO);
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity::ZERO);
    }
}
