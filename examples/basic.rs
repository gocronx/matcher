//! Minimal cross: two resting sells, one crossing buy, two trades printed.
//!
//! Run with:
//!     cargo run --example basic

use matcher::{Order, OrderBook, Side};

fn main() {
    let mut book = OrderBook::new();

    book.submit(Order::limit(1, Side::Sell, 100, 5), 0);
    book.submit(Order::limit(2, Side::Sell, 101, 5), 0);
    println!(
        "resting: best_ask={:?}, depth={}",
        book.best_ask(),
        book.len()
    );

    let trades = book.submit(Order::limit(3, Side::Buy, 101, 8), 1);

    println!("buy 3 @101 x8 produced {} trade(s):", trades.len());
    for t in &trades {
        println!(
            "  buy={} sell={} px={} qty={} aggressor={:?}",
            t.buy_id, t.sell_id, t.price, t.quantity, t.aggressor
        );
    }
    println!(
        "after: best_ask={:?}, depth={}",
        book.best_ask(),
        book.len()
    );
}
