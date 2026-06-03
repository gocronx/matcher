use crate::types::{
    AmendRejectReason, BookEvent, Order, OrderId, OrderType, Price, Quantity, Side,
};

use super::OrderBook;

// ---------------------------------------------------------------------------
// impl OrderBook — amend
// ---------------------------------------------------------------------------

impl OrderBook {
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

    /// Reduce order quantity for iceberg orders (reduces hidden reserves first).
    pub(super) fn reduce_order_quantity(order: &mut Order, new_qty: Quantity) {
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AmendRejectReason, BookEvent, Order, OrderId, Price, Quantity, Side};

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
