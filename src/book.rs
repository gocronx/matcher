use crate::types::{
    BookEvent, CancelRejectReason, Order, OrderId, OrderType, Price, Quantity, RejectReason, Side,
    Timestamp, Trade,
};
use ahash::AHashMap;
use smallvec::SmallVec;
use std::collections::BTreeMap;

struct PriceLevel {
    orders: SmallVec<[OrderId; 8]>,
    total_qty: Quantity,
}
impl PriceLevel {
    fn new() -> Self {
        Self {
            orders: SmallVec::new(),
            total_qty: 0,
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

    #[cfg(test)]
    fn assert_invariants(&self) {
        self.assert_invariants_at("book", 0, 0);
    }

    #[cfg(test)]
    fn assert_invariants_at(&self, scenario: &str, seed: u64, step: usize) {
        let mut seen = std::collections::HashSet::new();
        let mut expected_bid = None;
        let mut expected_ask = None;
        let context = || format!("{scenario} seed={seed} step={step}");

        for (&price, level) in &self.bids {
            assert!(
                !level.orders.is_empty(),
                "{}: empty bid level at {price}",
                context()
            );
            expected_bid = Some(price);
            let mut total = 0;
            for id in &level.orders {
                assert!(
                    seen.insert(*id),
                    "{}: order {id} appears in multiple levels",
                    context()
                );
                let order = self
                    .orders
                    .get(id)
                    .unwrap_or_else(|| panic!("{}: bid level references missing order", context()));
                assert_eq!(order.side, Side::Buy, "{}", context());
                assert_eq!(order.price, price, "{}", context());
                total += Self::level_qty(order);
            }
            assert_eq!(
                level.total_qty,
                total,
                "{}: bad bid total at {price}",
                context()
            );
        }

        for (&price, level) in &self.asks {
            assert!(
                !level.orders.is_empty(),
                "{}: empty ask level at {price}",
                context()
            );
            if expected_ask.is_none() {
                expected_ask = Some(price);
            }
            let mut total = 0;
            for id in &level.orders {
                assert!(
                    seen.insert(*id),
                    "{}: order {id} appears in multiple levels",
                    context()
                );
                let order = self
                    .orders
                    .get(id)
                    .unwrap_or_else(|| panic!("{}: ask level references missing order", context()));
                assert_eq!(order.side, Side::Sell, "{}", context());
                assert_eq!(order.price, price, "{}", context());
                total += Self::level_qty(order);
            }
            assert_eq!(
                level.total_qty,
                total,
                "{}: bad ask total at {price}",
                context()
            );
        }

        assert_eq!(self.best_bid, expected_bid, "{}", context());
        assert_eq!(self.best_ask, expected_ask, "{}", context());
        assert_eq!(self.orders.len(), seen.len(), "{}", context());
        if let (Some(bid), Some(ask)) = (self.best_bid, self.best_ask) {
            assert!(
                bid < ask,
                "{}: crossed book: bid={bid} ask={ask}",
                context()
            );
        }
    }

    fn iceberg_visible(kind: OrderType) -> Option<Quantity> {
        if let OrderType::Iceberg { visible } = kind {
            Some(visible)
        } else {
            None
        }
    }

    fn level_qty(o: &Order) -> Quantity {
        Self::iceberg_visible(o.kind).map_or(o.remaining(), |v| v.min(o.remaining()))
    }

    fn user_remaining(o: &Order) -> Quantity {
        o.remaining().saturating_add(o.hidden)
    }

    fn validate_order(&self, order: &Order) -> Option<RejectReason> {
        if self.orders.contains_key(&order.id) {
            return Some(RejectReason::DuplicateOrderId);
        }
        if order.quantity == 0 {
            return Some(RejectReason::InvalidQuantity);
        }
        if matches!(order.kind, OrderType::Iceberg { visible: 0 }) {
            return Some(RejectReason::InvalidQuantity);
        }
        if !matches!(order.kind, OrderType::Market) && order.price == 0 {
            return Some(RejectReason::InvalidPrice);
        }
        None
    }

    fn rest(&mut self, order: Order) {
        let (id, price, side, qty) = (order.id, order.price, order.side, Self::level_qty(&order));
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
        let (price, qty) = (o.price, Self::level_qty(&o));
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

    pub fn cancel(&mut self, id: OrderId) -> bool {
        self.remove_resting_order(id).is_some()
    }

    pub fn cancel_events(&mut self, id: OrderId) -> Vec<BookEvent> {
        match self.remove_resting_order(id) {
            Some(order) => vec![BookEvent::Canceled {
                order_id: id,
                remaining: Self::user_remaining(&order),
            }],
            None => vec![BookEvent::CancelRejected {
                order_id: id,
                reason: CancelRejectReason::UnknownOrderId,
            }],
        }
    }

    fn can_fill(&self, order: &Order) -> bool {
        let mut avail: Quantity = 0;
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
            if incoming.remaining() == 0 {
                break;
            }
            let ids: SmallVec<[OrderId; 8]> = match aggressor {
                Side::Buy => self.asks.get(&price).map(|l| l.orders.clone()),
                Side::Sell => self.bids.get(&price).map(|l| l.orders.clone()),
            }
            .unwrap_or_default();

            for rest_id in ids {
                if incoming.remaining() == 0 {
                    break;
                }
                let Some(rest) = self.orders.get_mut(&rest_id) else {
                    continue;
                };
                let fill = incoming.remaining().min(rest.remaining());
                if fill == 0 {
                    continue;
                }
                rest.filled += fill;
                let (rfilled, rkind, rhidden, rside, rprice) = (
                    rest.is_filled(),
                    rest.kind,
                    rest.hidden,
                    rest.side,
                    rest.price,
                );
                incoming.filled += fill;

                let (buy_id, sell_id) = if aggressor == Side::Buy {
                    (incoming.id, rest_id)
                } else {
                    (rest_id, incoming.id)
                };
                events.push(BookEvent::Trade(Trade {
                    buy_id,
                    sell_id,
                    price,
                    quantity: fill,
                    ts: now,
                    aggressor,
                }));

                if rfilled {
                    let levels = match rside {
                        Side::Buy => &mut self.bids,
                        Side::Sell => &mut self.asks,
                    };
                    if let Some(lvl) = levels.get_mut(&rprice) {
                        lvl.remove(rest_id, fill);
                        if lvl.orders.is_empty() {
                            self.drop_level(rside, rprice);
                        }
                    }
                    self.orders.remove(&rest_id);
                    if let Some(vis_sz) = Self::iceberg_visible(rkind) {
                        if rhidden > 0 {
                            let refill = rhidden.min(vis_sz);
                            self.rest(Order {
                                id: rest_id,
                                side: rside,
                                kind: rkind,
                                price: rprice,
                                quantity: refill,
                                filled: 0,
                                hidden: rhidden - refill,
                            });
                            if let Some(rested) = self.orders.get(&rest_id) {
                                events.push(BookEvent::Rested {
                                    order_id: rest_id,
                                    remaining: Self::user_remaining(rested),
                                });
                            }
                        }
                    }
                } else {
                    let levels = match rside {
                        Side::Buy => &mut self.bids,
                        Side::Sell => &mut self.asks,
                    };
                    if let Some(lvl) = levels.get_mut(&rprice) {
                        lvl.total_qty = lvl.total_qty.saturating_sub(fill);
                    }
                    if let Some(rest) = self.orders.get(&rest_id) {
                        events.push(BookEvent::Rested {
                            order_id: rest_id,
                            remaining: Self::user_remaining(rest),
                        });
                    }
                }
            }
        }
    }

    pub fn submit(&mut self, order: Order, now: Timestamp) -> Vec<Trade> {
        self.submit_events(order, now)
            .into_iter()
            .filter_map(|event| match event {
                BookEvent::Trade(trade) => Some(trade),
                _ => None,
            })
            .collect()
    }

    pub fn submit_events(&mut self, mut order: Order, now: Timestamp) -> Vec<BookEvent> {
        let mut events = Vec::new();

        if let Some(reason) = self.validate_order(&order) {
            return vec![BookEvent::Rejected {
                order_id: order.id,
                reason,
            }];
        }

        if matches!(order.kind, OrderType::PostOnly) {
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
            return events;
        }

        if matches!(order.kind, OrderType::Fok) && !self.can_fill(&order) {
            return vec![BookEvent::Rejected {
                order_id: order.id,
                reason: RejectReason::FokNotFillable,
            }];
        }

        events.push(BookEvent::Accepted { order_id: order.id });
        self.match_side(&mut order, now, &mut events);

        if order.remaining() > 0 {
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
                    order.filled = 0;
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
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BookEvent, CancelRejectReason, Order, RejectReason, Side};
    use std::collections::BTreeSet;

    fn lim(id: OrderId, side: Side, price: Price, qty: Quantity) -> Order {
        Order::limit(id, side, price, qty)
    }
    fn mkt(id: OrderId, side: Side, qty: Quantity) -> Order {
        Order::market(id, side, qty)
    }

    fn assert_trade(
        event: &BookEvent,
        buy_id: OrderId,
        sell_id: OrderId,
        price: Price,
        quantity: Quantity,
    ) {
        let BookEvent::Trade(trade) = event else {
            panic!("expected trade event, got {event:?}");
        };
        assert_eq!(trade.buy_id, buy_id);
        assert_eq!(trade.sell_id, sell_id);
        assert_eq!(trade.price, price);
        assert_eq!(trade.quantity, quantity);
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
        assert_eq!(b.best_bid(), Some(100));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn market_order_matches_fifo_at_price_level() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        b.submit(lim(2, Side::Sell, 100, 5), 0);
        let t = b.submit(mkt(3, Side::Buy, 8), 1);
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].sell_id, t[0].quantity), (1, 5));
        assert_eq!((t[1].sell_id, t[1].quantity), (2, 3));
    }

    #[test]
    fn ioc_drops_remainder() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 3), 0);
        let t = b.submit(Order::ioc(2, Side::Buy, 100, 10), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, 3);
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
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
    }

    #[test]
    fn iceberg_refills_after_visible_fills() {
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        let t = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, 10);
        assert_eq!(b.best_ask(), Some(100));
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
    }

    #[test]
    fn iceberg_refill_path_preserves_book_invariants_after_each_visible_fill() {
        let mut b = OrderBook::new();
        b.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
        b.assert_invariants();

        let first = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(first.len(), 1);
        assert_eq!(b.best_ask(), Some(100));
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
        b.assert_invariants();

        let second = b.submit(mkt(3, Side::Buy, 10), 2);
        assert_eq!(second.len(), 1);
        assert_eq!(b.best_ask(), Some(100));
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
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
        assert_eq!(events[0], BookEvent::Accepted { order_id: 3 });
        assert_trade(&events[1], 3, 1, 100, 5);
        assert_trade(&events[2], 3, 2, 101, 3);
        assert_eq!(
            events[3],
            BookEvent::Rested {
                order_id: 2,
                remaining: 2,
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
                order_id: 2,
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
                order_id: 1,
                reason: RejectReason::DuplicateOrderId,
            }]
        );
        let trades = b.submit(mkt(2, Side::Buy, 10), 2);
        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].price, 100);
        assert_eq!(trades[0].quantity, 5);
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn cancel_events_reports_success_and_unknown_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Buy, 100, 10), 0);

        assert_eq!(
            b.cancel_events(1),
            vec![BookEvent::Canceled {
                order_id: 1,
                remaining: 10,
            }]
        );
        assert_eq!(
            b.cancel_events(1),
            vec![BookEvent::CancelRejected {
                order_id: 1,
                reason: CancelRejectReason::UnknownOrderId,
            }]
        );
    }

    #[test]
    fn randomized_submit_cancel_flow_preserves_book_invariants() {
        let mut b = OrderBook::new();
        let mut next_id = 1;
        let mut resting_ids = BTreeSet::new();

        for step in 0..512 {
            if step % 7 == 0 && !resting_ids.is_empty() {
                let idx = (step * 31 + 11) % resting_ids.len();
                let id = *resting_ids.iter().nth(idx).expect("resting id");
                resting_ids.remove(&id);
                b.cancel(id);
            } else {
                let side = if step % 2 == 0 { Side::Buy } else { Side::Sell };
                let price = 95 + ((step * 17) % 11) as Price;
                let qty = 1 + ((step * 13) % 5) as Quantity;
                let id = next_id;
                next_id += 1;

                let events = b.submit_events(Order::limit(id, side, price, qty), step as Timestamp);
                remember_new_resting_order(&mut resting_ids, id, &events);
            }

            b.assert_invariants_at("randomized_limit_flow", 0, step);
            resting_ids.retain(|id| b.orders.contains_key(id));
        }
    }

    #[test]
    fn seeded_mixed_order_flow_preserves_book_invariants() {
        for seed in [1, 7, 19, 73] {
            let mut rng = Lcg::new(seed);
            let mut b = OrderBook::new();
            let mut next_id = 1;
            let mut resting_ids = BTreeSet::new();

            for step in 0..384 {
                if rng.range(9) == 0 && !resting_ids.is_empty() {
                    let idx = rng.range(resting_ids.len() as u64) as usize;
                    let id = *resting_ids.iter().nth(idx).expect("resting id");
                    resting_ids.remove(&id);
                    b.cancel(id);
                } else {
                    let id = next_id;
                    next_id += 1;
                    let side = if rng.range(2) == 0 {
                        Side::Buy
                    } else {
                        Side::Sell
                    };
                    let price = 95 + rng.range(11) as Price;
                    let qty = 1 + rng.range(8) as Quantity;
                    let order = match rng.range(6) {
                        0 => Order::limit(id, side, price, qty),
                        1 => Order::ioc(id, side, price, qty),
                        2 => Order::fok(id, side, price, qty),
                        3 => Order::post_only(id, side, price, qty),
                        4 => {
                            let visible = 1 + rng.range(qty) as Quantity;
                            Order::iceberg(id, side, price, qty + visible, visible)
                        }
                        _ => Order::market(id, side, qty),
                    };

                    let events = b.submit_events(order, step as Timestamp);
                    remember_new_resting_order(&mut resting_ids, id, &events);
                }

                b.assert_invariants_at("seeded_mixed_order_flow", seed, step);
                resting_ids.retain(|id| b.orders.contains_key(id));
            }
        }
    }
}
