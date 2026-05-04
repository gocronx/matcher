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
                let (rfilled, rkind, rhidden, rside, rprice, rqty) = (
                    rest.is_filled(),
                    rest.kind,
                    rest.hidden,
                    rest.side,
                    rest.price,
                    rest.quantity,
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
                    let vis = Self::iceberg_visible(rkind).map_or(rqty, |v| v.min(rqty));
                    let levels = match rside {
                        Side::Buy => &mut self.bids,
                        Side::Sell => &mut self.asks,
                    };
                    if let Some(lvl) = levels.get_mut(&rprice) {
                        lvl.remove(rest_id, vis);
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
    use crate::types::{BookEvent, CancelRejectReason, Order, OrderType, RejectReason, Side};

    fn lim(id: OrderId, side: Side, price: Price, qty: Quantity) -> Order {
        Order {
            id,
            side,
            kind: OrderType::Limit,
            price,
            quantity: qty,
            filled: 0,
            hidden: 0,
        }
    }
    fn mkt(id: OrderId, side: Side, qty: Quantity) -> Order {
        Order {
            id,
            side,
            kind: OrderType::Market,
            price: 0,
            quantity: qty,
            filled: 0,
            hidden: 0,
        }
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
        let t = b.submit(
            Order {
                id: 2,
                side: Side::Buy,
                kind: OrderType::Ioc,
                price: 100,
                quantity: 10,
                filled: 0,
                hidden: 0,
            },
            1,
        );
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, 3);
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn fok_rejects_when_partial_only() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 3), 0);
        let t = b.submit(
            Order {
                id: 2,
                side: Side::Buy,
                kind: OrderType::Fok,
                price: 100,
                quantity: 10,
                filled: 0,
                hidden: 0,
            },
            1,
        );
        assert_eq!(t.len(), 0);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn post_only_rejects_crossing_order() {
        let mut b = OrderBook::new();
        b.submit(lim(1, Side::Sell, 100, 5), 0);
        let t = b.submit(
            Order {
                id: 2,
                side: Side::Buy,
                kind: OrderType::PostOnly,
                price: 100,
                quantity: 5,
                filled: 0,
                hidden: 0,
            },
            1,
        );
        assert_eq!(t.len(), 0);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn iceberg_only_visible_quantity_in_level() {
        let mut b = OrderBook::new();
        b.submit(
            Order {
                id: 1,
                side: Side::Sell,
                kind: OrderType::Iceberg { visible: 10 },
                price: 100,
                quantity: 10,
                filled: 0,
                hidden: 90,
            },
            0,
        );
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
    }

    #[test]
    fn iceberg_refills_after_visible_fills() {
        let mut b = OrderBook::new();
        b.submit(
            Order {
                id: 1,
                side: Side::Sell,
                kind: OrderType::Iceberg { visible: 10 },
                price: 100,
                quantity: 10,
                filled: 0,
                hidden: 20,
            },
            0,
        );
        let t = b.submit(mkt(2, Side::Buy, 10), 1);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].quantity, 10);
        assert_eq!(b.best_ask(), Some(100));
        assert_eq!(b.asks.get(&100).map(|l| l.total_qty), Some(10));
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

        let events = b.submit_events(
            Order {
                id: 2,
                side: Side::Buy,
                kind: OrderType::PostOnly,
                price: 100,
                quantity: 5,
                filled: 0,
                hidden: 0,
            },
            1,
        );

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
}
