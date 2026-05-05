//! End-to-end smoke test: spawn the matcher actor over a pair of in-memory
//! channels, push a few orders through, observe trades come back.

use matcher::book::OrderBook;
use matcher::codec::{decode_inbound, decode_trade, encode_cancel, encode_submit, Inbound};
use matcher::matcher as engine;
use matcher::types::{Order, OrderId, Price, Quantity, Side, Trade};
use matcher::{BookEvent, RejectReason};
use tokio::sync::mpsc;

#[test]
fn cross_via_book_directly() {
    let mut book = OrderBook::new();
    book.submit(Order::limit(1, Side::Sell, 100, 5), 0);
    book.submit(Order::limit(2, Side::Sell, 101, 5), 0);

    let trades = book.submit(Order::limit(3, Side::Buy, 101, 8), 1);
    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].quantity, Quantity(5));
    assert_eq!(trades[0].price, Price(100)); // walks the cheapest ask first
    assert_eq!(trades[1].quantity, Quantity(3));
    assert_eq!(trades[1].price, Price(101));
    assert_eq!(book.best_ask(), Some(Price(101))); // 2 left at 101
}

#[test]
fn codec_round_trip_through_actor() {
    // Drive the matcher actor through a tokio runtime without networking:
    // feed encoded packets through a channel and observe trade output.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        let (in_tx, in_rx) = mpsc::channel::<Inbound>(8);
        let (trade_tx, mut trade_rx) = mpsc::channel::<Trade>(8);

        let actor = tokio::spawn(engine::run(in_rx, trade_tx, || matcher::Timestamp(42)));

        // Submit a sell, then a crossing buy. Verify the round-trip via the
        // wire codec to confirm the protocol matches the engine's view.
        for order in [
            Order::limit(1, Side::Sell, 50, 4),
            Order::limit(2, Side::Buy, 50, 4),
        ] {
            let buf = encode_submit(&order);
            let Inbound::Submit(decoded) = decode_inbound(&buf).unwrap() else {
                panic!("expected Submit");
            };
            in_tx.send(Inbound::Submit(decoded)).await.unwrap();
        }

        let trade = trade_rx.recv().await.expect("trade expected");
        assert_eq!(trade.buy_id, OrderId(2));
        assert_eq!(trade.sell_id, OrderId(1));
        assert_eq!(trade.quantity, Quantity(4));
        assert_eq!(trade.price, Price(50));

        // Verify the trade also round-trips through the wire codec.
        let buf = matcher::codec::encode_trade(&trade);
        let back = decode_trade(&buf).unwrap();
        assert_eq!(back.buy_id, trade.buy_id);
        assert_eq!(back.aggressor, trade.aggressor);

        // Submitting a cancel for an order that doesn't exist is a no-op.
        let cancel_buf = encode_cancel(999u64);
        let Inbound::Cancel(id) = decode_inbound(&cancel_buf).unwrap() else {
            panic!("expected Cancel");
        };
        in_tx.send(Inbound::Cancel(id)).await.unwrap();

        drop(in_tx);
        actor.await.unwrap();
    });
}

#[test]
fn event_api_is_available_from_crate_root() {
    let mut book = OrderBook::new();

    let events = book.submit_events(Order::limit(1, Side::Buy, 100, 10), 0);

    assert_eq!(
        events[0],
        BookEvent::Accepted {
            order_id: OrderId(1)
        }
    );
    assert_eq!(
        book.submit_events(Order::limit(1, Side::Buy, 101, 10), 1),
        vec![BookEvent::Rejected {
            order_id: OrderId(1),
            reason: RejectReason::DuplicateOrderId,
        }]
    );
}
