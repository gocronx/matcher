//! Iceberg refill: only the visible slice sits in the level; hidden refills it.
//!
//! Run with:
//!     cargo run --example iceberg

use matcher::{Order, OrderBook, Price, Side};

fn main() {
    let mut book = OrderBook::new();

    // Sell iceberg: total 30, visible 10. Only 10 shows on the book.
    book.submit(Order::iceberg(1, Side::Sell, 100, 30, 10), 0);
    print_book("after iceberg rests", &book);

    // Each market buy of 10 fully consumes the visible slice and triggers a refill
    // until hidden is exhausted.
    for (taker_id, ts) in [(2, 1), (3, 2), (4, 3)] {
        let trades = book.submit(Order::market(taker_id, Side::Buy, 10), ts);
        println!(
            "market buy id={taker_id} x10 -> {} trade(s) @ px={}",
            trades.len(),
            trades.first().map(|t| t.price).unwrap_or(Price(0)),
        );
        print_book("  after fill", &book);
    }
}

fn print_book(label: &str, book: &OrderBook) {
    println!(
        "{label}: best_ask={:?}, depth={}",
        book.best_ask(),
        book.len()
    );
}
