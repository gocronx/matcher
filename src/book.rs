use crate::types::{
    AmendRejectReason, BookEvent, CancelRejectReason, Order, OrderId, OrderType, Price, Quantity,
    RejectReason, Side, Timestamp, Trade,
};
use ahash::AHashMap;
use smallvec::SmallVec;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Snapshot / restore
// ---------------------------------------------------------------------------

/// Error returned by [`OrderBook::load`] when bytes are malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotError {
    /// The leading 8-byte magic does not match `b"MATCHER\x01"`.
    BadMagic,
    /// The 4-byte version field holds a value this build does not support.
    UnsupportedVersion(u32),
    /// The byte slice ends before all declared records could be parsed.
    Truncated,
    /// A record's side byte is not 1 (Buy) or 2 (Sell).
    InvalidSide(u8),
    /// A record's kind_tag byte is not 2 (Limit), 5 (PostOnly), or 6 (Iceberg).
    InvalidKind(u8),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::BadMagic => write!(f, "snapshot: bad magic bytes"),
            SnapshotError::UnsupportedVersion(v) => {
                write!(f, "snapshot: unsupported version {v}")
            }
            SnapshotError::Truncated => write!(f, "snapshot: payload truncated"),
            SnapshotError::InvalidSide(b) => write!(f, "snapshot: invalid side byte {b}"),
            SnapshotError::InvalidKind(b) => write!(f, "snapshot: invalid kind byte {b}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

// Magic = b"MATCHER\x01" (8 bytes)
const SNAP_MAGIC: &[u8; 8] = b"MATCHER\x01";
const SNAP_VERSION: u32 = 1;

// Byte offsets within the fixed header.
const HDR_MAGIC_END: usize = 8;
const HDR_VERSION_END: usize = 12;
const HDR_NORDERS_END: usize = 20;
const HEADER_LEN: usize = 20;

// Per-record encoding constants (reuse codec side/kind tags).
const SIDE_BUY: u8 = 1;
const SIDE_SELL: u8 = 2;
const KIND_LIMIT: u8 = 2;
const KIND_POST_ONLY: u8 = 5;
const KIND_ICEBERG: u8 = 6;

// Fixed bytes for a single record (before the optional iceberg_visible u64).
const RECORD_BASE: usize = 49;

/// Aggregates the per-match context passed to `fill_one_resting`.
struct MatchContext {
    price: Price,
    now: Timestamp,
    aggressor: Side,
}

/// Configuration for `OrderBook::check_side_invariants` (test/fuzzing only).
#[cfg(any(test, feature = "fuzzing"))]
struct SideConfig {
    expected_side: Side,
    /// true for asks (ascending iteration → first = best); false for bids (last = best).
    first_is_best: bool,
}

/// Snapshot of a resting order's fields captured before it is mutated/removed.
/// Avoids borrowing `self.orders` while also mutating the book.
struct RestingSnap {
    id: OrderId,
    side: Side,
    kind: OrderType,
    price: Price,
    hidden: Quantity,
    filled: bool,
}

impl RestingSnap {
    fn from_order(o: &Order, id: OrderId) -> Self {
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

struct PriceLevel {
    orders: SmallVec<[OrderId; 8]>,
    total_qty: Quantity,
}
impl PriceLevel {
    fn new() -> Self {
        Self {
            orders: SmallVec::new(),
            total_qty: Quantity::ZERO,
        }
    }
    fn add(&mut self, id: OrderId, qty: Quantity) {
        self.orders.push(id);
        self.total_qty += qty;
    }
    fn remove(&mut self, id: OrderId, qty: Quantity) {
        if let Some(p) = self.orders.iter().position(|&x| x == id) {
            self.orders.remove(p);
            self.total_qty = self.total_qty.saturating_sub(qty);
        }
    }
}

pub struct OrderBook {
    orders: AHashMap<OrderId, Order>,
    bids: BTreeMap<Price, PriceLevel>,
    asks: BTreeMap<Price, PriceLevel>,
    best_bid: Option<Price>,
    best_ask: Option<Price>,
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
    fn assert_invariants(&self) {
        self.assert_invariants_at("book", 0, 0);
    }

    #[cfg(any(test, feature = "fuzzing"))]
    fn assert_invariants_at(&self, scenario: &str, seed: u64, step: usize) {
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

    fn iceberg_visible(kind: OrderType) -> Option<Quantity> {
        if let OrderType::Iceberg { visible } = kind {
            Some(visible)
        } else {
            None
        }
    }

    fn order_level_qty(o: &Order) -> Quantity {
        Self::iceberg_visible(o.kind).map_or(o.remaining(), |v| v.min(o.remaining()))
    }

    fn user_remaining(o: &Order) -> Quantity {
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

    fn rest(&mut self, order: Order) {
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

    fn drop_level(&mut self, side: Side, price: Price) {
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

    /// Amend an existing resting order's price and/or quantity.
    ///
    /// Rules:
    /// - Only resting limit/post-only/iceberg orders can be amended
    /// - Price changes lose time priority (order moves to back of queue at new price)
    /// - Price changes that would cross the book are rejected
    /// - Quantity can only be decreased (increases are rejected)
    /// - Quantity decrease maintains time priority
    pub fn amend(
        &mut self,
        id: impl Into<OrderId>,
        new_price: Option<Price>,
        new_quantity: Option<Quantity>,
    ) -> Vec<BookEvent> {
        let id = id.into();

        if new_price.is_none() && new_quantity.is_none() {
            return vec![];
        }

        if let Some(reject) = self.validate_amend(id, new_price, new_quantity) {
            return vec![reject];
        }

        if new_price.is_some() {
            self.apply_amend_price_change(id, new_price, new_quantity)
        } else {
            self.apply_amend_qty_only(id, new_quantity.unwrap())
        }
    }

    /// Validate an amend request; returns a rejection event if invalid.
    fn validate_amend(
        &self,
        id: OrderId,
        new_price: Option<Price>,
        new_quantity: Option<Quantity>,
    ) -> Option<BookEvent> {
        let Some(order) = self.orders.get(&id) else {
            return Some(BookEvent::AmendRejected {
                order_id: id,
                reason: AmendRejectReason::UnknownOrderId,
            });
        };

        if !matches!(
            order.kind,
            OrderType::Limit | OrderType::PostOnly | OrderType::Iceberg { .. }
        ) {
            return Some(BookEvent::AmendRejected {
                order_id: id,
                reason: AmendRejectReason::OrderTypeNotAmendable,
            });
        }

        if let Some(price) = new_price {
            if price == Price::ZERO {
                return Some(BookEvent::AmendRejected {
                    order_id: id,
                    reason: AmendRejectReason::InvalidPrice,
                });
            }
            let would_cross = match order.side {
                Side::Buy => self.best_ask.is_some_and(|a| price >= a),
                Side::Sell => self.best_bid.is_some_and(|b| price <= b),
            };
            if would_cross {
                return Some(BookEvent::AmendRejected {
                    order_id: id,
                    reason: AmendRejectReason::WouldCross,
                });
            }
        }

        if let Some(qty) = new_quantity {
            if qty == Quantity::ZERO {
                return Some(BookEvent::AmendRejected {
                    order_id: id,
                    reason: AmendRejectReason::InvalidQuantity,
                });
            }
            if qty > Self::user_remaining(order) {
                return Some(BookEvent::AmendRejected {
                    order_id: id,
                    reason: AmendRejectReason::QuantityIncrease,
                });
            }
        }

        None
    }

    /// Apply an amend where the price changes: remove, update, re-insert (loses time priority).
    fn apply_amend_price_change(
        &mut self,
        id: OrderId,
        new_price: Option<Price>,
        new_quantity: Option<Quantity>,
    ) -> Vec<BookEvent> {
        let mut order = self.remove_resting_order(id).unwrap();
        order.price = new_price.unwrap();
        if let Some(new_qty) = new_quantity {
            Self::reduce_order_quantity(&mut order, new_qty);
        }
        let final_qty = Self::user_remaining(&order);
        self.rest(order);
        vec![BookEvent::Amended {
            order_id: id,
            new_price,
            new_quantity: final_qty,
        }]
    }

    /// Apply a quantity-only amend: modify in place to maintain time priority.
    fn apply_amend_qty_only(&mut self, id: OrderId, new_qty: Quantity) -> Vec<BookEvent> {
        let order = self.orders.get_mut(&id).unwrap();
        let old_visible_qty = Self::order_level_qty(order);
        Self::reduce_order_quantity(order, new_qty);
        let visible_delta = old_visible_qty.saturating_sub(Self::order_level_qty(order));

        let (side, price) = (order.side, order.price);
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        if let Some(lvl) = levels.get_mut(&price) {
            lvl.total_qty = lvl.total_qty.saturating_sub(visible_delta);
        }

        vec![BookEvent::Amended {
            order_id: id,
            new_price: None,
            new_quantity: new_qty,
        }]
    }

    /// Helper to reduce order quantity (for iceberg orders, reduces hidden first)
    fn reduce_order_quantity(order: &mut Order, new_qty: Quantity) {
        let current_total = Self::user_remaining(order);
        let reduction = current_total.saturating_sub(new_qty);

        if reduction >= order.hidden {
            // Reduce all hidden and some visible
            let visible_reduction = reduction.saturating_sub(order.hidden);
            order.hidden = Quantity::ZERO;
            order.quantity = order.quantity.saturating_sub(visible_reduction);
        } else {
            // Only reduce hidden
            order.hidden = order.hidden.saturating_sub(reduction);
        }
    }

    fn can_fill(&self, order: &Order) -> bool {
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

    fn price_list(&self, incoming: &Order) -> Vec<Price> {
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

    fn match_side(&mut self, incoming: &mut Order, now: Timestamp, events: &mut Vec<BookEvent>) {
        let aggressor = incoming.side;
        for price in self.price_list(incoming) {
            if incoming.remaining() == Quantity::ZERO {
                break;
            }
            let ids: SmallVec<[OrderId; 8]> = match aggressor {
                Side::Buy => self.asks.get(&price).map(|l| l.orders.clone()),
                Side::Sell => self.bids.get(&price).map(|l| l.orders.clone()),
            }
            .unwrap_or_default();

            for rest_id in ids {
                if incoming.remaining() == Quantity::ZERO {
                    break;
                }
                self.fill_one_resting(
                    incoming,
                    rest_id,
                    MatchContext {
                        price,
                        now,
                        aggressor,
                    },
                    events,
                );
            }
        }
    }

    /// Fill a single resting order against the incoming aggressor, then handle
    /// level-quantity bookkeeping and iceberg replenishment.
    fn fill_one_resting(
        &mut self,
        incoming: &mut Order,
        rest_id: OrderId,
        ctx: MatchContext,
        events: &mut Vec<BookEvent>,
    ) {
        let Some(rest) = self.orders.get_mut(&rest_id) else {
            return;
        };
        let fill = incoming.remaining().min(rest.remaining());
        if fill == Quantity::ZERO {
            return;
        }
        rest.filled += fill;
        let snap = RestingSnap::from_order(rest, rest_id);
        incoming.filled += fill;

        let (buy_id, sell_id) = if ctx.aggressor == Side::Buy {
            (incoming.id, rest_id)
        } else {
            (rest_id, incoming.id)
        };
        events.push(BookEvent::Trade(Trade {
            buy_id,
            sell_id,
            price: ctx.price,
            quantity: fill,
            ts: ctx.now,
            aggressor: ctx.aggressor,
        }));

        if snap.filled {
            self.remove_filled_resting(&snap, fill, events);
        } else {
            self.update_partial_resting(&snap, fill, events);
        }
    }

    /// Remove a fully-filled resting order from the level index and handle
    /// iceberg replenishment.
    fn remove_filled_resting(
        &mut self,
        snap: &RestingSnap,
        fill: Quantity,
        events: &mut Vec<BookEvent>,
    ) {
        let levels = match snap.side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        if let Some(lvl) = levels.get_mut(&snap.price) {
            lvl.remove(snap.id, fill);
            if lvl.orders.is_empty() {
                self.drop_level(snap.side, snap.price);
            }
        }
        self.orders.remove(&snap.id);
        if let Some(vis_sz) = Self::iceberg_visible(snap.kind) {
            if snap.hidden > Quantity::ZERO {
                let refill = snap.hidden.min(vis_sz);
                self.rest(Order {
                    id: snap.id,
                    side: snap.side,
                    kind: snap.kind,
                    price: snap.price,
                    quantity: refill,
                    filled: Quantity::ZERO,
                    hidden: snap.hidden - refill,
                });
                if let Some(rested) = self.orders.get(&snap.id) {
                    events.push(BookEvent::Rested {
                        order_id: snap.id,
                        remaining: Self::user_remaining(rested),
                    });
                }
            }
        }
    }

    /// Update level quantity for a partially-filled resting order and emit Rested.
    fn update_partial_resting(
        &mut self,
        snap: &RestingSnap,
        fill: Quantity,
        events: &mut Vec<BookEvent>,
    ) {
        let levels = match snap.side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        if let Some(lvl) = levels.get_mut(&snap.price) {
            lvl.total_qty = lvl.total_qty.saturating_sub(fill);
        }
        if let Some(rest) = self.orders.get(&snap.id) {
            events.push(BookEvent::Rested {
                order_id: snap.id,
                remaining: Self::user_remaining(rest),
            });
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

    /// Handle a post-only order: reject if it would cross, otherwise accept and rest.
    fn handle_post_only(&mut self, order: Order, events: &mut Vec<BookEvent>) -> Vec<BookEvent> {
        let crosses = match order.side {
            Side::Buy => self.best_ask.is_some_and(|a| order.price >= a),
            Side::Sell => self.best_bid.is_some_and(|b| order.price <= b),
        };
        if crosses {
            return vec![BookEvent::Rejected {
                order_id: order.id,
                reason: RejectReason::PostOnlyWouldCross,
            }];
        }
        events.push(BookEvent::Accepted { order_id: order.id });
        let remaining = Self::user_remaining(&order);
        let order_id = order.id;
        self.rest(order);
        events.push(BookEvent::Rested {
            order_id,
            remaining,
        });
        std::mem::take(events)
    }

    /// Rest any unfilled quantity after matching (Limit and Iceberg only).
    fn rest_unfilled_remainder(&mut self, mut order: Order, events: &mut Vec<BookEvent>) {
        if order.remaining() == Quantity::ZERO {
            return;
        }
        match order.kind {
            OrderType::Limit => {
                let order_id = order.id;
                let remaining = Self::user_remaining(&order);
                self.rest(order);
                events.push(BookEvent::Rested {
                    order_id,
                    remaining,
                });
            }
            OrderType::Iceberg { visible } => {
                let total = order.remaining() + order.hidden;
                let v = visible.min(total);
                order.quantity = v;
                order.filled = Quantity::ZERO;
                order.hidden = total - v;
                let order_id = order.id;
                self.rest(order);
                events.push(BookEvent::Rested {
                    order_id,
                    remaining: total,
                });
            }
            _ => {}
        }
    }

    // -----------------------------------------------------------------------
    // Snapshot / restore
    // -----------------------------------------------------------------------

    /// Serialize the entire book to a self-describing binary blob.
    ///
    /// Round-tripping through [`OrderBook::load`] reproduces an identical book:
    /// same orders, same per-level FIFO time priority, same best_bid/best_ask.
    pub fn snapshot(&self) -> Vec<u8> {
        // Upper-bound capacity: header + worst-case all icebergs (57 bytes each).
        let n = self.orders.len();
        let mut buf = Vec::with_capacity(HEADER_LEN + n * (RECORD_BASE + 8));

        // Header
        buf.extend_from_slice(SNAP_MAGIC);
        buf.extend_from_slice(&SNAP_VERSION.to_be_bytes());
        buf.extend_from_slice(&(n as u64).to_be_bytes());

        // Emit orders in FIFO order per level.
        // Bids: ascending price (lowest first). Each level's orders list is
        // already in submission (FIFO) order.
        for level in self.bids.values() {
            for &id in &level.orders {
                if let Some(o) = self.orders.get(&id) {
                    Self::write_order_record(&mut buf, o);
                }
            }
        }
        // Asks: ascending price.
        for level in self.asks.values() {
            for &id in &level.orders {
                if let Some(o) = self.orders.get(&id) {
                    Self::write_order_record(&mut buf, o);
                }
            }
        }

        buf
    }

    fn write_order_record(buf: &mut Vec<u8>, o: &Order) {
        // [0]    side
        buf.push(match o.side {
            Side::Buy => SIDE_BUY,
            Side::Sell => SIDE_SELL,
        });
        // [1]    kind_tag
        let kind_tag = match o.kind {
            OrderType::Limit => KIND_LIMIT,
            OrderType::PostOnly => KIND_POST_ONLY,
            OrderType::Iceberg { .. } => KIND_ICEBERG,
            // Market/IOC/FOK never rest; unreachable in practice.
            _ => unreachable!("non-resting order kind in snapshot"),
        };
        buf.push(kind_tag);
        // [2..8] reserved (6 zero bytes)
        buf.extend_from_slice(&[0u8; 6]);
        // [8..16]  id
        buf.extend_from_slice(&o.id.0.to_be_bytes());
        // [16..24] price
        buf.extend_from_slice(&o.price.0.to_be_bytes());
        // [24..32] quantity
        buf.extend_from_slice(&o.quantity.0.to_be_bytes());
        // [32..40] filled
        buf.extend_from_slice(&o.filled.0.to_be_bytes());
        // [40..48] hidden
        buf.extend_from_slice(&o.hidden.0.to_be_bytes());
        // [48]     iceberg_visible_set (0 or 1)
        if let OrderType::Iceberg { visible } = o.kind {
            buf.push(1u8);
            // +8 bytes: iceberg_visible
            buf.extend_from_slice(&visible.0.to_be_bytes());
        } else {
            buf.push(0u8);
        }
    }

    /// Reconstruct an [`OrderBook`] from bytes produced by [`OrderBook::snapshot`].
    ///
    /// Returns `Err` if the magic/version mismatches or the bytes are malformed.
    pub fn load(bytes: &[u8]) -> Result<Self, SnapshotError> {
        let n_orders = Self::parse_snapshot_header(bytes)?;
        let mut book = OrderBook::new();
        let mut pos = HEADER_LEN;
        for _ in 0..n_orders {
            let (order, consumed) = Self::parse_order_record(&bytes[pos..], bytes.len() - pos)?;
            pos += consumed;
            // Bypass submit_events (which would match!) — directly rest the order.
            book.rest(order);
        }
        Ok(book)
    }

    /// Validate and parse the fixed 20-byte snapshot header; returns the order count.
    fn parse_snapshot_header(bytes: &[u8]) -> Result<usize, SnapshotError> {
        // Magic check: compare however many bytes we have against the magic
        // prefix. If they differ we know it's wrong magic (not just truncation).
        // Only report Truncated when the bytes so far do match the prefix.
        let magic_cmp_len = bytes.len().min(HDR_MAGIC_END);
        if bytes[..magic_cmp_len] != SNAP_MAGIC[..magic_cmp_len] {
            return Err(SnapshotError::BadMagic);
        }
        if bytes.len() < HEADER_LEN {
            return Err(SnapshotError::Truncated);
        }
        let version = u32::from_be_bytes(
            bytes[HDR_MAGIC_END..HDR_VERSION_END]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        if version != SNAP_VERSION {
            return Err(SnapshotError::UnsupportedVersion(version));
        }
        let n_orders = u64::from_be_bytes(
            bytes[HDR_VERSION_END..HDR_NORDERS_END]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        ) as usize;
        Ok(n_orders)
    }

    /// Parse one order record from `record_bytes` (a slice starting at the record).
    /// `remaining_len` is the number of bytes left in the full buffer (for bounds checks).
    /// Returns `(Order, bytes_consumed)`.
    ///
    /// Long but flat: 9 fixed-offset field reads followed by two enum decodings.
    /// Extracting sub-functions would require passing partially-parsed state across
    /// call boundaries, making the wire-format contract harder to verify by reading.
    #[allow(clippy::too_many_lines)]
    fn parse_order_record(
        record_bytes: &[u8],
        remaining_len: usize,
    ) -> Result<(Order, usize), SnapshotError> {
        if remaining_len < RECORD_BASE {
            return Err(SnapshotError::Truncated);
        }

        let side_byte = record_bytes[0];
        let kind_tag = record_bytes[1];
        // [2..8] reserved — skip
        let id = u64::from_be_bytes(
            record_bytes[8..16]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let price = u64::from_be_bytes(
            record_bytes[16..24]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let quantity = u64::from_be_bytes(
            record_bytes[24..32]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let filled = u64::from_be_bytes(
            record_bytes[32..40]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let hidden = u64::from_be_bytes(
            record_bytes[40..48]
                .try_into()
                .map_err(|_| SnapshotError::Truncated)?,
        );
        let iceberg_visible_set = record_bytes[48];
        let mut consumed = RECORD_BASE;

        let side = match side_byte {
            SIDE_BUY => Side::Buy,
            SIDE_SELL => Side::Sell,
            other => return Err(SnapshotError::InvalidSide(other)),
        };

        let kind = match kind_tag {
            KIND_LIMIT => OrderType::Limit,
            KIND_POST_ONLY => OrderType::PostOnly,
            KIND_ICEBERG => {
                if iceberg_visible_set != 0 {
                    if remaining_len < RECORD_BASE + 8 {
                        return Err(SnapshotError::Truncated);
                    }
                    let vis = u64::from_be_bytes(
                        record_bytes[RECORD_BASE..RECORD_BASE + 8]
                            .try_into()
                            .map_err(|_| SnapshotError::Truncated)?,
                    );
                    consumed += 8;
                    OrderType::Iceberg {
                        visible: Quantity(vis),
                    }
                } else {
                    // iceberg_visible_set == 0: fall back to quantity as the visible slice.
                    OrderType::Iceberg {
                        visible: Quantity(quantity),
                    }
                }
            }
            other => return Err(SnapshotError::InvalidKind(other)),
        };

        let order = Order {
            id: OrderId(id),
            side,
            kind,
            price: Price(price),
            quantity: Quantity(quantity),
            filled: Quantity(filled),
            hidden: Quantity(hidden),
        };
        Ok((order, consumed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BookEvent, CancelRejectReason, Order, OrderId, Price, Quantity, RejectReason, Side,
    };
    use std::collections::BTreeSet;

    fn lim(
        id: impl Into<OrderId>,
        side: Side,
        price: impl Into<Price>,
        qty: impl Into<Quantity>,
    ) -> Order {
        Order::limit(id, side, price, qty)
    }
    fn mkt(id: impl Into<OrderId>, side: Side, qty: impl Into<Quantity>) -> Order {
        Order::market(id, side, qty)
    }

    fn assert_trade(
        event: &BookEvent,
        buy_id: impl Into<OrderId>,
        sell_id: impl Into<OrderId>,
        price: impl Into<Price>,
        quantity: impl Into<Quantity>,
    ) {
        let BookEvent::Trade(trade) = event else {
            panic!("expected trade event, got {event:?}");
        };
        assert_eq!(trade.buy_id, buy_id.into());
        assert_eq!(trade.sell_id, sell_id.into());
        assert_eq!(trade.price, price.into());
        assert_eq!(trade.quantity, quantity.into());
    }

    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            self.0
        }

        fn range(&mut self, upper: u64) -> u64 {
            debug_assert_ne!(upper, 0, "range upper bound must be non-zero");
            self.next() % upper
        }
    }

    fn remember_new_resting_order(
        resting_ids: &mut BTreeSet<OrderId>,
        id: OrderId,
        events: &[BookEvent],
    ) {
        if events
            .iter()
            .any(|event| matches!(event, BookEvent::Rested { order_id, .. } if *order_id == id))
        {
            resting_ids.insert(id);
        }
    }

    #[test]
    fn limit_order_rests_on_book() {
        let mut b = OrderBook::new();
        assert_eq!(b.submit(lim(1, Side::Buy, 100, 10), 0).len(), 0);
        assert_eq!(b.best_bid(), Some(Price(100)));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn market_order_matches_fifo_at_price_level() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 100, 5), 0);
        let t = b.submit(mkt(3, Side::Buy, 8), 1);
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].sell_id, t[0].quantity), (OrderId(1), Quantity(5)));
        assert_eq!((t[1].sell_id, t[1].quantity), (OrderId(2), Quantity(3)));
    }

    #[test]
    fn ioc_drops_remainder() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 3), 0);
        let t = b.submit(Order::ioc(2, Side::Buy, 100, 10), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, Quantity(3));
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn fok_rejects_when_partial_only() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 3), 0);
        let t = b.submit(Order::fok(2, Side::Buy, 100, 10), 1);
        assert_eq!(t.len(), 0);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn post_only_rejects_crossing_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        let t = b.submit(Order::post_only(2, Side::Buy, 100, 5), 1);
        assert_eq!(t.len(), 0);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn iceberg_only_visible_quantity_in_level() {
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 100, 10), 0);
        assert_eq!(
            b.asks.get(&Price(100)).map(|l| l.total_qty),
            Some(Quantity(10))
        );
    }

    #[test]
    fn iceberg_refills_after_visible_fills() {
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        let t = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, Quantity(10));
        assert_eq!(b.best_ask(), Some(Price(100)));
        assert_eq!(
            b.asks.get(&Price(100)).map(|l| l.total_qty),
            Some(Quantity(10))
        );
    }

    #[test]
    fn iceberg_refill_path_preserves_book_invariants_after_each_visible_fill() {
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        b.assert_invariants();

        let first = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(first.len(), 1);
        assert_eq!(b.best_ask(), Some(Price(100)));
        assert_eq!(
            b.asks.get(&Price(100)).map(|l| l.total_qty),
            Some(Quantity(10))
        );
        b.assert_invariants();

        let second = b.submit(mkt(3, Side::Buy, 10), 2);
        assert_eq!(second.len(), 1);
        assert_eq!(b.best_ask(), Some(Price(100)));
        assert_eq!(
            b.asks.get(&Price(100)).map(|l| l.total_qty),
            Some(Quantity(10))
        );
        b.assert_invariants();

        let third = b.submit(mkt(4, Side::Buy, 10), 3);
        assert_eq!(third.len(), 1);
        assert_eq!(b.best_ask(), None);
        assert_eq!(b.len(), 0);
        b.assert_invariants();
    }

    #[test]
    fn cancel_removes_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        assert!(b.cancel(1));
        assert_eq!(b.len(), 0);
        assert_eq!(b.best_bid(), None);
        assert!(!b.cancel(1));
    }

    #[test]
    fn submit_events_reports_accept_trades_and_resting_remainder() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 101, 5), 0);

        let events = b.submit_events(lim(3, Side::Buy, 101, 8), 1);

        assert_eq!(events.len(), 4);
        assert_eq!(
            events[0],
            BookEvent::Accepted {
                order_id: OrderId(3)
            }
        );
        assert_trade(&events[1], 3u64, 1u64, 100u64, 5u64);
        assert_trade(&events[2], 3u64, 2u64, 101u64, 3u64);
        assert_eq!(
            events[3],
            BookEvent::Rested {
                order_id: OrderId(2),
                remaining: Quantity(2),
            }
        );
    }

    #[test]
    fn submit_events_reports_post_only_rejection() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);

        let events = b.submit_events(Order::post_only(2, Side::Buy, 100, 5), 1);

        assert_eq!(
            events,
            vec![BookEvent::Rejected {
                order_id: OrderId(2),
                reason: RejectReason::PostOnlyWouldCross,
            }]
        );
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn submit_events_rejects_duplicate_order_id_without_corrupting_book() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);

        let events = b.submit_events(lim(1, Side::Sell, 101, 7), 1);

        assert_eq!(
            events,
            vec![BookEvent::Rejected {
                order_id: OrderId(1),
                reason: RejectReason::DuplicateOrderId,
            }]
        );
        let trades = b.submit(mkt(2, Side::Buy, 10), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].price, Price(100));
        assert_eq!(trades[0].quantity, Quantity(5));
        assert_eq!(b.len(), 0);
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
    fn randomized_submit_cancel_flow_preserves_book_invariants() {
        let mut b = OrderBook::new();
        let mut next_id: u64 = 1;
        let mut resting_ids = BTreeSet::new();

        for step in 0..512usize {
            if step % 7 == 0 && !resting_ids.is_empty() {
                let idx = (step * 31 + 11) % resting_ids.len();
                let id = *resting_ids.iter().nth(idx).expect("resting id");
                resting_ids.remove(&id);
                b.cancel(id);
            } else {
                let side = if step % 2 == 0 { Side::Buy } else { Side::Sell };
                let price = Price(95 + ((step * 17) % 11) as u64);
                let qty = Quantity(1 + ((step * 13) % 5) as u64);
                let id = OrderId(next_id);
                next_id += 1;

                let events = b.submit_events(Order::limit(id, side, price, qty), step as u64);
                remember_new_resting_order(&mut resting_ids, id, &events);
            }

            b.assert_invariants_at("randomized_limit_flow", 0, step);
            resting_ids.retain(|id| b.orders.contains_key(id));
        }
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

    #[test]
    fn seeded_mixed_order_flow_preserves_book_invariants() {
        for seed in [1u64, 7, 19, 73] {
            let mut rng = Lcg::new(seed);
            let mut b = OrderBook::new();
            let mut next_id: u64 = 1;
            let mut resting_ids = BTreeSet::new();

            for step in 0..384usize {
                if rng.range(9) == 0 && !resting_ids.is_empty() {
                    let idx = rng.range(resting_ids.len() as u64) as usize;
                    let id = *resting_ids.iter().nth(idx).expect("resting id");
                    resting_ids.remove(&id);
                    b.cancel(id);
                } else {
                    let id = OrderId(next_id);
                    next_id += 1;
                    let side = if rng.range(2) == 0 {
                        Side::Buy
                    } else {
                        Side::Sell
                    };
                    let price = Price(95 + rng.range(11));
                    let qty = Quantity(1 + rng.range(8));
                    let order = match rng.range(6) {
                        0 => Order::limit(id, side, price, qty),
                        1 => Order::ioc(id, side, price, qty),
                        2 => Order::fok(id, side, price, qty),
                        3 => Order::post_only(id, side, price, qty),
                        4 => {
                            let visible = Quantity(1 + rng.range(qty.0));
                            Order::iceberg(id, side, price, qty + visible, visible)
                        }
                        _ => Order::market(id, side, qty),
                    };

                    let events = b.submit_events(order, step as u64);
                    remember_new_resting_order(&mut resting_ids, id, &events);
                }

                b.assert_invariants_at("seeded_mixed_order_flow", seed, step);
                resting_ids.retain(|id| b.orders.contains_key(id));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Snapshot / restore tests
    // -----------------------------------------------------------------------

    #[test]
    fn snapshot_empty_book_round_trips() {
        let b = OrderBook::new();
        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");
        assert_eq!(b2.len(), 0);
        assert_eq!(b2.best_bid(), None);
        assert_eq!(b2.best_ask(), None);
        b2.assert_invariants();
    }

    #[test]
    fn snapshot_limit_orders_round_trip_preserves_state() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        b.submit(lim(2, Side::Buy, 99, 5), 0);
        b.submit(lim(3, Side::Sell, 101, 8), 0);
        b.submit(lim(4, Side::Sell, 102, 3), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");

        assert_eq!(b2.len(), 4);
        assert_eq!(b2.best_bid(), Some(Price(100)));
        assert_eq!(b2.best_ask(), Some(Price(101)));
        assert_eq!(b2.level_qty(Side::Buy, 100u64), Quantity(10));
        assert_eq!(b2.level_qty(Side::Buy, 99u64), Quantity(5));
        assert_eq!(b2.level_qty(Side::Sell, 101u64), Quantity(8));
        assert_eq!(b2.level_qty(Side::Sell, 102u64), Quantity(3));
        b2.assert_invariants();
    }

    #[test]
    fn snapshot_preserves_fifo_priority() {
        // Two sell limits at the same price, older order first.
        let mut b = OrderBook::new();
        b.submit(lim(10, Side::Sell, 100, 5), 0); // older
        b.submit(lim(11, Side::Sell, 100, 5), 1); // newer
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // A buy market taker should trade with the older sell (id=10) first.
        let trades = b2.submit(mkt(99, Side::Buy, 5), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(
            trades[0].sell_id,
            OrderId(10),
            "FIFO violated: newer order traded first"
        );
        assert_eq!(trades[0].quantity, Quantity(5));
        // Older order fully consumed; newer still rests.
        assert_eq!(b2.len(), 1);
    }

    #[test]
    fn snapshot_iceberg_preserves_hidden_quantity() {
        // Iceberg: total=30, visible=10, hidden=20.
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // Drain the visible chunk — should refill from hidden.
        let t1 = b2.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t1.len(), 1);
        assert_eq!(t1[0].quantity, Quantity(10));
        assert_eq!(b2.best_ask(), Some(Price(100)), "iceberg did not refill");

        // Drain second visible chunk.
        let t2 = b2.submit(mkt(3, Side::Buy, 10), 2);
        assert_eq!(t2.len(), 1);
        assert_eq!(t2[0].quantity, Quantity(10));
        assert_eq!(
            b2.best_ask(),
            Some(Price(100)),
            "iceberg did not refill again"
        );

        // Drain final chunk — book should be empty.
        let t3 = b2.submit(mkt(4, Side::Buy, 10), 3);
        assert_eq!(t3.len(), 1);
        assert_eq!(t3[0].quantity, Quantity(10));
        assert_eq!(b2.best_ask(), None);
        assert_eq!(b2.len(), 0);
    }

    #[test]
    fn snapshot_partial_fills_preserve_filled() {
        // Submit a limit qty=10, partially fill 3, then snapshot.
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 10), 0);
        // Partial fill: taker buys 3.
        let trades = b.submit(mkt(2, Side::Buy, 3), 1);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, Quantity(3));
        // Resting sell order now has remaining=7.
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(7));
        b.assert_invariants();

        let snap = b.snapshot();
        let mut b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        // Remaining should still be 7 (i.e. quantity=7 with filled=0 after
        // partial fill was captured in the snapshot as quantity=7, filled=3
        // — but order_level_qty uses remaining() = quantity - filled).
        assert_eq!(b2.level_qty(Side::Sell, 100u64), Quantity(7));

        // Drain the remaining 7.
        let t = b2.submit(mkt(3, Side::Buy, 7), 2);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, Quantity(7));
        assert_eq!(b2.len(), 0);
    }

    #[test]
    fn load_rejects_bad_magic() {
        let result = OrderBook::load(b"BADMAGIC");
        assert!(
            matches!(result, Err(SnapshotError::BadMagic)),
            "expected BadMagic, got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn load_rejects_unsupported_version() {
        // Craft a header with valid magic but version=99.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"MATCHER\x01"); // magic
        bytes.extend_from_slice(&99u32.to_be_bytes()); // version=99
        bytes.extend_from_slice(&0u64.to_be_bytes()); // n_orders=0
        let result = OrderBook::load(&bytes);
        assert!(
            matches!(result, Err(SnapshotError::UnsupportedVersion(99))),
            "expected UnsupportedVersion(99), got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn load_rejects_truncated_payload() {
        // Build a valid 1-order snapshot, then truncate the last few bytes.
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        let snap = b.snapshot();

        // Truncate: drop the last 5 bytes — cuts into the last record.
        let truncated = &snap[..snap.len() - 5];
        let result = OrderBook::load(truncated);
        assert!(
            matches!(result, Err(SnapshotError::Truncated)),
            "expected Truncated, got {result:?}",
            result = result.map(|_| "<book>"),
        );
    }

    #[test]
    fn snapshot_then_load_passes_assert_invariants() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        b.submit(lim(2, Side::Buy, 100, 5), 0); // same level, FIFO
        b.submit(lim(3, Side::Sell, 101, 7), 0);
        b.submit(Order::post_only(4, Side::Sell, 102, 3), 0);
        b.submit(Order::iceberg(5, Side::Sell, 103, 50, 10), 0);
        b.assert_invariants();

        let snap = b.snapshot();
        let b2 = OrderBook::load(&snap).expect("load failed");
        b2.assert_invariants();

        assert_eq!(b2.len(), b.len());
        assert_eq!(b2.best_bid(), b.best_bid());
        assert_eq!(b2.best_ask(), b.best_ask());
    }

    // -----------------------------------------------------------------------
    // Order amendment tests
    // -----------------------------------------------------------------------

    #[test]
    fn amend_price_loses_time_priority() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0); // older
        b.submit(lim(2, Side::Sell, 100, 5), 1); // newer
        b.assert_invariants();

        // Amend order 1's price to 101 (should move to back of queue)
        let events = b.amend(1u64, Some(Price(101)), None);
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Amended {
                order_id,
                new_price,
                new_quantity,
            } => {
                assert_eq!(order_id, OrderId(1));
                assert_eq!(new_price, Some(Price(101)));
                assert_eq!(new_quantity, Quantity(5));
            }
            _ => panic!("expected Amended event"),
        }

        // Now order 2 should trade first (it's still at 100)
        let trades = b.submit(mkt(3, Side::Buy, 5), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].sell_id, OrderId(2), "FIFO violated after amend");
        assert_eq!(trades[0].price, Price(100));
    }

    #[test]
    fn amend_quantity_decrease_maintains_time_priority() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 10), 0); // older
        b.submit(lim(2, Side::Sell, 100, 5), 1); // newer
        b.assert_invariants();

        // Reduce order 1's quantity from 10 to 3
        let events = b.amend(1u64, None, Some(Quantity(3)));
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Amended {
                order_id,
                new_price,
                new_quantity,
            } => {
                assert_eq!(order_id, OrderId(1));
                assert_eq!(new_price, None);
                assert_eq!(new_quantity, Quantity(3));
            }
            _ => panic!("expected Amended event"),
        }

        // Order 1 should still trade first (time priority maintained)
        let trades = b.submit(mkt(3, Side::Buy, 3), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(
            trades[0].sell_id,
            OrderId(1),
            "time priority lost on qty decrease"
        );
    }

    #[test]
    fn amend_rejects_quantity_increase() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        let events = b.amend(1u64, None, Some(Quantity(15)));
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(1),
                reason: AmendRejectReason::QuantityIncrease,
            }]
        );

        // Order should still be on book unchanged
        assert_eq!(b.len(), 1);
        assert_eq!(b.level_qty(Side::Buy, 100u64), Quantity(10));
    }

    #[test]
    fn amend_rejects_unknown_order() {
        let mut b = OrderBook::new();
        let events = b.amend(999u64, Some(Price(100)), None);
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(999),
                reason: AmendRejectReason::UnknownOrderId,
            }]
        );
    }

    #[test]
    fn amend_rejects_zero_price() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        let events = b.amend(1u64, Some(Price(0)), None);
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(1),
                reason: AmendRejectReason::InvalidPrice,
            }]
        );
    }

    #[test]
    fn amend_rejects_zero_quantity() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        let events = b.amend(1u64, None, Some(Quantity(0)));
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(1),
                reason: AmendRejectReason::InvalidQuantity,
            }]
        );
    }

    #[test]
    fn amend_both_price_and_quantity() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 10), 0);
        b.assert_invariants();

        let events = b.amend(1u64, Some(Price(101)), Some(Quantity(5)));
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Amended {
                order_id,
                new_price,
                new_quantity,
            } => {
                assert_eq!(order_id, OrderId(1));
                assert_eq!(new_price, Some(Price(101)));
                assert_eq!(new_quantity, Quantity(5));
            }
            _ => panic!("expected Amended event"),
        }

        assert_eq!(b.best_ask(), Some(Price(101)));
        assert_eq!(b.level_qty(Side::Sell, 101u64), Quantity(5));
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity::ZERO);
        b.assert_invariants();
    }

    #[test]
    fn amend_iceberg_reduces_hidden_first() {
        let mut b = OrderBook::new();
        // Iceberg: total=30, visible=10, hidden=20
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(10));

        // Reduce to 25 (should reduce hidden from 20 to 15)
        let events = b.amend(1u64, None, Some(Quantity(25)));
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Amended { new_quantity, .. } => {
                assert_eq!(new_quantity, Quantity(25));
            }
            _ => panic!("expected Amended event"),
        }

        // Visible should still be 10
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(10));

        // Trade away visible portion - should refill to 10 from remaining 15 hidden
        let t1 = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t1.len(), 1);
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(10));

        // Trade away second visible portion - should refill to 5 (last of hidden)
        let t2 = b.submit(mkt(3, Side::Buy, 10), 2);
        assert_eq!(t2.len(), 1);
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(5));

        // Trade away final portion
        let t3 = b.submit(mkt(4, Side::Buy, 5), 3);
        assert_eq!(t3.len(), 1);
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn amend_iceberg_reduces_visible_when_hidden_exhausted() {
        let mut b = OrderBook::new();
        // Iceberg: total=30, visible=10, hidden=20
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);

        // Reduce to 5 (reduce hidden 20 + visible 5)
        let events = b.amend(1u64, None, Some(Quantity(5)));
        assert_eq!(events.len(), 1);

        // Visible should now be 5
        assert_eq!(b.level_qty(Side::Sell, 100u64), Quantity(5));

        // Trade should consume all 5 and book should be empty
        let trades = b.submit(mkt(2, Side::Buy, 5), 1);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, Quantity(5));
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn amend_preserves_book_invariants() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);
        b.submit(lim(2, Side::Buy, 99, 5), 0);
        b.submit(lim(3, Side::Sell, 101, 8), 0);
        b.assert_invariants();

        // Amend buy order price
        b.amend(1u64, Some(Price(98)), None);
        b.assert_invariants();

        // Amend sell order quantity
        b.amend(3u64, None, Some(Quantity(5)));
        b.assert_invariants();

        // Amend both
        b.amend(2u64, Some(Price(97)), Some(Quantity(3)));
        b.assert_invariants();
    }

    #[test]
    fn amend_post_only_order_works() {
        let mut b = OrderBook::new();
        b.submit(Order::post_only(1, Side::Buy, 100, 10), 0);

        let events = b.amend(1u64, Some(Price(99)), Some(Quantity(5)));
        assert_eq!(events.len(), 1);
        match events[0] {
            BookEvent::Amended {
                order_id,
                new_price,
                new_quantity,
            } => {
                assert_eq!(order_id, OrderId(1));
                assert_eq!(new_price, Some(Price(99)));
                assert_eq!(new_quantity, Quantity(5));
            }
            _ => panic!("expected Amended event"),
        }
    }

    #[test]
    fn amend_rejects_crossing_buy_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 110, 5), 1);
        b.assert_invariants();

        // Try to amend buy order to cross the ask
        let events = b.amend(1u64, Some(Price(120)), None);
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(1),
                reason: AmendRejectReason::WouldCross,
            }]
        );

        // Book should be unchanged
        assert_eq!(b.best_bid(), Some(Price(100)));
        assert_eq!(b.best_ask(), Some(Price(110)));
        b.assert_invariants();
    }

    #[test]
    fn amend_rejects_crossing_sell_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 110, 5), 1);
        b.assert_invariants();

        // Try to amend sell order to cross the bid
        let events = b.amend(2u64, Some(Price(90)), None);
        assert_eq!(
            events,
            vec![BookEvent::AmendRejected {
                order_id: OrderId(2),
                reason: AmendRejectReason::WouldCross,
            }]
        );

        // Book should be unchanged
        assert_eq!(b.best_bid(), Some(Price(100)));
        assert_eq!(b.best_ask(), Some(Price(110)));
        b.assert_invariants();
    }

    #[test]
    fn amend_allows_non_crossing_price_changes() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 110, 5), 1);
        b.assert_invariants();

        // Amend buy to 105 (still below ask of 110)
        let events = b.amend(1u64, Some(Price(105)), None);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], BookEvent::Amended { .. }));
        assert_eq!(b.best_bid(), Some(Price(105)));
        b.assert_invariants();

        // Amend sell to 108 (still above bid of 105)
        let events = b.amend(2u64, Some(Price(108)), None);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], BookEvent::Amended { .. }));
        assert_eq!(b.best_ask(), Some(Price(108)));
        b.assert_invariants();
    }

    #[test]
    fn amend_with_no_changes_is_noop() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        let events = b.amend(1u64, None, None);
        assert_eq!(events.len(), 0);
        assert_eq!(b.len(), 1);
    }
}
