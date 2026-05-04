use criterion::{black_box, criterion_group, criterion_main, Criterion};
use matcher::{Order, OrderBook, Side};

/// Build a fresh book with `n` non-crossing sell limit orders resting at
/// distinct prices (101, 102, …, 100+n) and return the book together with
/// the order IDs that were submitted.
fn prefilled_sells(n: u64) -> (OrderBook, Vec<u64>) {
    let mut book = OrderBook::new();
    let ids: Vec<u64> = (1..=n).collect();
    for &id in &ids {
        // Sells at prices 101, 102, …  — no buy side, so nothing crosses.
        book.submit(Order::limit(id, Side::Sell, 100 + id, 10), id);
    }
    (book, ids)
}

// ---------------------------------------------------------------------------
// Bench 1: insert N non-crossing limit buy orders (no matches)
// ---------------------------------------------------------------------------
fn submit_limit_no_match(c: &mut Criterion) {
    const N: u64 = 1_000;

    c.bench_function("submit_limit_no_match", |b| {
        b.iter_with_setup(
            || {
                let book = OrderBook::new();
                // Buy orders at prices 1, 2, …, N — all below any sell, so
                // none will cross each other or a non-existent ask side.
                let orders: Vec<Order> = (1..=N)
                    .map(|id| Order::limit(id, Side::Buy, id, 10))
                    .collect();
                (book, orders)
            },
            |(mut book, orders)| {
                for order in orders {
                    black_box(book.submit(black_box(order), 0));
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Bench 2: single market-buy that sweeps all 1 000 resting sells
// ---------------------------------------------------------------------------
fn submit_market_full_sweep(c: &mut Criterion) {
    const N: u64 = 1_000;
    // Each sell rests with qty=10, so we need N*10 to consume them all.
    const SWEEP_QTY: u64 = N * 10;

    c.bench_function("submit_market_full_sweep", |b| {
        b.iter_with_setup(
            || {
                let (book, _) = prefilled_sells(N);
                // The market order id must not collide with the 1..=N sell ids.
                let mkt = Order::market(N + 1, Side::Buy, SWEEP_QTY);
                (book, mkt)
            },
            |(mut book, mkt)| {
                black_box(book.submit(black_box(mkt), 1));
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Bench 3: cancel every order in a 1 000-order book one by one
// ---------------------------------------------------------------------------
fn cancel_random(c: &mut Criterion) {
    const N: u64 = 1_000;

    c.bench_function("cancel_random", |b| {
        b.iter_with_setup(
            || prefilled_sells(N),
            |(mut book, ids)| {
                for id in ids {
                    black_box(book.cancel(black_box(id)));
                }
            },
        );
    });
}

criterion_group!(
    benches,
    submit_limit_no_match,
    submit_market_full_sweep,
    cancel_random
);
criterion_main!(benches);
