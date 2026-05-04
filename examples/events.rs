//! Full event stream: accepts, trades, rests, rejects, cancels.
//!
//! Run with:
//!     cargo run --example events

use matcher::{BookEvent, Order, OrderBook, Side};

fn main() {
    let mut book = OrderBook::new();

    print_events(
        "submit sell 1 @100 x5",
        book.submit_events(Order::limit(1, Side::Sell, 100, 5), 0),
    );
    print_events(
        "submit sell 2 @101 x5",
        book.submit_events(Order::limit(2, Side::Sell, 101, 5), 1),
    );

    // Crosses both levels, partial fills the second resting order.
    print_events(
        "submit buy 3 @101 x8 (crosses)",
        book.submit_events(Order::limit(3, Side::Buy, 101, 8), 2),
    );

    // Duplicate id rejected without touching book state.
    print_events(
        "submit dup id 2 (rejected)",
        book.submit_events(Order::limit(2, Side::Buy, 99, 1), 3),
    );

    // Cancel the partially filled rest of order 2.
    print_events("cancel id 2", book.cancel_events(2));

    // Cancel an unknown id is also surfaced as an event.
    print_events("cancel id 999 (unknown)", book.cancel_events(999));
}

fn print_events(label: &str, events: Vec<BookEvent>) {
    println!("\n[{label}]");
    for e in events {
        match e {
            BookEvent::Accepted { order_id } => println!("  accepted id={order_id}"),
            BookEvent::Trade(t) => println!(
                "  trade buy={} sell={} px={} qty={}",
                t.buy_id, t.sell_id, t.price, t.quantity
            ),
            BookEvent::Rested {
                order_id,
                remaining,
            } => {
                println!("  rested id={order_id} remaining={remaining}")
            }
            BookEvent::Canceled {
                order_id,
                remaining,
            } => {
                println!("  canceled id={order_id} remaining={remaining}")
            }
            BookEvent::Rejected { order_id, reason } => {
                println!("  rejected id={order_id} reason={reason:?}")
            }
            BookEvent::CancelRejected { order_id, reason } => {
                println!("  cancel_rejected id={order_id} reason={reason:?}")
            }
            _ => println!("  (unknown event variant)"),
        }
    }
}
