//! One pass over each order type: Limit, Market, IOC, FOK, PostOnly.
//!
//! Run with:
//!     cargo run --example order_types

use matcher::{BookEvent, Order, OrderBook, Side};

fn main() {
    println!("--- Limit: rests when it can't cross ---");
    {
        let mut book = OrderBook::new();
        let events = book.submit_events(Order::limit(1, Side::Buy, 100, 5), 0);
        summarize(&events);
    }

    println!("\n--- Market: takes whatever is available ---");
    {
        let mut book = OrderBook::new();
        book.submit(Order::limit(1, Side::Sell, 100, 3), 0);
        book.submit(Order::limit(2, Side::Sell, 101, 3), 0);
        let events = book.submit_events(Order::market(3, Side::Buy, 5), 1);
        summarize(&events);
    }

    println!("\n--- IOC: fills what it can, drops the rest ---");
    {
        let mut book = OrderBook::new();
        book.submit(Order::limit(1, Side::Sell, 100, 3), 0);
        let events = book.submit_events(Order::ioc(2, Side::Buy, 100, 10), 1);
        summarize(&events);
        println!(
            "  book depth after IOC = {} (no resting remainder)",
            book.len()
        );
    }

    println!("\n--- FOK: rejected unless fully fillable ---");
    {
        let mut book = OrderBook::new();
        book.submit(Order::limit(1, Side::Sell, 100, 3), 0);
        let events = book.submit_events(Order::fok(2, Side::Buy, 100, 10), 1);
        summarize(&events);
    }

    println!("\n--- PostOnly: rejected if it would take liquidity ---");
    {
        let mut book = OrderBook::new();
        book.submit(Order::limit(1, Side::Sell, 100, 5), 0);
        let events = book.submit_events(Order::post_only(2, Side::Buy, 100, 5), 1);
        summarize(&events);
    }
}

fn summarize(events: &[BookEvent]) {
    for e in events {
        match e {
            BookEvent::Accepted { order_id } => println!("  accepted id={order_id}"),
            BookEvent::Trade(t) => println!("  trade px={} qty={}", t.price, t.quantity),
            BookEvent::Rested {
                order_id,
                remaining,
            } => {
                println!("  rested id={order_id} remaining={remaining}")
            }
            BookEvent::Rejected { order_id, reason } => {
                println!("  rejected id={order_id} reason={reason:?}")
            }
            BookEvent::Canceled {
                order_id,
                remaining,
            } => {
                println!("  canceled id={order_id} remaining={remaining}")
            }
            BookEvent::CancelRejected { order_id, reason } => {
                println!("  cancel_rejected id={order_id} reason={reason:?}")
            }
        }
    }
}
