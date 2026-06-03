use crate::types::{
    BookEvent, Order, OrderId, OrderType, Quantity, RejectReason, Side, Timestamp, Trade,
};
use smallvec::SmallVec;

use super::{MatchContext, OrderBook, RestingSnap};

// ---------------------------------------------------------------------------
// impl OrderBook — matching engine
// ---------------------------------------------------------------------------

impl OrderBook {
    pub(super) fn match_side(
        &mut self,
        incoming: &mut Order,
        now: Timestamp,
        events: &mut Vec<BookEvent>,
    ) {
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

    /// Handle a post-only order: reject if it would cross, otherwise accept and rest.
    pub(super) fn handle_post_only(
        &mut self,
        order: Order,
        events: &mut Vec<BookEvent>,
    ) -> Vec<BookEvent> {
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
    pub(super) fn rest_unfilled_remainder(
        &mut self,
        mut order: Order,
        events: &mut Vec<BookEvent>,
    ) {
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BookEvent, Order, OrderId, Price, Quantity, RejectReason, Side};
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
}
