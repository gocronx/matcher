use criterion::{black_box, criterion_group, criterion_main, Criterion};
use matcher::{Order, OrderBook, Side};
use std::time::{Duration, Instant};

/// Build a fresh book with `n` non-crossing sell limit orders resting at
/// distinct prices (101, 102, …, 100+n).
fn prefilled_sells(n: u64) -> (OrderBook, Vec<u64>) {
    let mut book = OrderBook::new();
    let ids: Vec<u64> = (1..=n).collect();
    for &id in &ids {
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
    const SWEEP_QTY: u64 = N * 10;

    c.bench_function("submit_market_full_sweep", |b| {
        b.iter_with_setup(
            || {
                let (book, _) = prefilled_sells(N);
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
// (Each order at a distinct price level, so cancel is dominated by
// BTreeMap traversal, not the per-level linear scan.)
// ---------------------------------------------------------------------------
fn cancel_random(c: &mut Criterion) {
    const N: u64 = 1_000;
    c.bench_function("cancel_random", |b| {
        b.iter_with_setup(
            || {
                let (book, mut ids) = prefilled_sells(N);
                shuffle(&mut ids, 0xCAFEBABE);
                (book, ids)
            },
            |(mut book, ids)| {
                for id in ids {
                    black_box(book.cancel(black_box(id)));
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Bench 4: 1 000 orders all at the SAME price level, cancelled in random
// order. This is the worst case for `PriceLevel::remove`'s O(n) linear
// scan — total work is O(n²).
// ---------------------------------------------------------------------------
fn cancel_same_price_stack(c: &mut Criterion) {
    const N: u64 = 1_000;
    c.bench_function("cancel_same_price_stack", |b| {
        b.iter_with_setup(
            || {
                let mut book = OrderBook::new();
                let mut ids: Vec<u64> = (1..=N).collect();
                for &id in &ids {
                    book.submit(Order::limit(id, Side::Sell, 100, 1), id);
                }
                shuffle(&mut ids, 0xDEADBEEF);
                (book, ids)
            },
            |(mut book, ids)| {
                for id in ids {
                    black_box(book.cancel(black_box(id)));
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Bench 5: insert one new limit into a 100 000-deep book.
//
// Uses iter_custom so the heavy setup is explicitly excluded from timing.
// The book grows by one per timed iteration; depth fluctuates around 100k
// across criterion's sample loop, but the per-op cost stays representative.
// ---------------------------------------------------------------------------
fn submit_into_deep_book(c: &mut Criterion) {
    const DEPTH: u64 = 100_000;
    c.bench_function("submit_into_deep_book", |b| {
        b.iter_custom(|iters| {
            let mut book = OrderBook::new();
            for id in 1..=DEPTH {
                book.submit(Order::limit(id, Side::Sell, 100_000 + id, 1), id);
            }

            let mut total = Duration::ZERO;
            for next_id in (DEPTH + 1)..(DEPTH + 1 + iters) {
                // Buy below any ask -> rests immediately, no match.
                let order = Order::limit(next_id, Side::Buy, 50_000, 1);

                let start = Instant::now();
                black_box(book.submit(black_box(order), 0));
                total += start.elapsed();
            }
            total
        });
    });
}

// ---------------------------------------------------------------------------
// Bench 6: cancel one mid-depth order from a 100 000-deep book.
//
// Same iter_custom strategy. Pre-fills DEPTH + iters orders so we can
// cancel a different mid-range ID each timed iteration.
// ---------------------------------------------------------------------------
fn cancel_in_deep_book(c: &mut Criterion) {
    const DEPTH: u64 = 100_000;
    c.bench_function("cancel_in_deep_book", |b| {
        b.iter_custom(|iters| {
            let total_orders = DEPTH + iters;
            let mut book = OrderBook::new();
            for id in 1..=total_orders {
                book.submit(Order::limit(id, Side::Sell, 100_000 + id, 1), id);
            }
            let start_id = total_orders / 2;

            let mut total = Duration::ZERO;
            for i in 0..iters {
                let id = start_id + i;
                let now = Instant::now();
                black_box(book.cancel(black_box(id)));
                total += now.elapsed();
            }
            total
        });
    });
}

// ---------------------------------------------------------------------------
// Bench 7: 80 / 15 / 5 mix of submit / cancel / market, deterministic.
//
// Op stream pre-generated outside the timed loop so we measure pure
// matcher dispatch, not the LCG.
// ---------------------------------------------------------------------------
fn mixed_workload_throughput(c: &mut Criterion) {
    const STEPS: u64 = 1_000;
    c.bench_function("mixed_workload_throughput", |b| {
        b.iter_with_setup(
            || {
                let mut ops: Vec<Op> = Vec::with_capacity(STEPS as usize);
                let mut rng = Lcg(0xDEADBEEF);
                let mut next_id: u64 = 1;
                let mut resting_ids: Vec<u64> = Vec::new();
                for _ in 0..STEPS {
                    let pick = rng.range(100);
                    if pick < 80 {
                        let side = pick_side(&mut rng);
                        let price = 100 + rng.range(21);
                        let qty = 1 + rng.range(5);
                        let id = next_id;
                        next_id += 1;
                        resting_ids.push(id);
                        ops.push(Op::Submit(Order::limit(id, side, price, qty)));
                    } else if pick < 95 && !resting_ids.is_empty() {
                        let idx = rng.range(resting_ids.len() as u64) as usize;
                        let id = resting_ids.swap_remove(idx);
                        ops.push(Op::Cancel(id));
                    } else {
                        let side = pick_side(&mut rng);
                        let qty = 1 + rng.range(50);
                        let id = next_id;
                        next_id += 1;
                        ops.push(Op::Submit(Order::market(id, side, qty)));
                    }
                }
                (OrderBook::new(), ops)
            },
            |(mut book, ops)| {
                for op in ops {
                    match op {
                        Op::Submit(o) => {
                            black_box(book.submit(black_box(o), 0));
                        }
                        Op::Cancel(id) => {
                            black_box(book.cancel(black_box(id)));
                        }
                    }
                }
            },
        );
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

enum Op {
    Submit(Order),
    Cancel(u64),
}

struct Lcg(u64);

impl Lcg {
    fn range(&mut self, upper: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0 % upper
    }
}

fn pick_side(rng: &mut Lcg) -> Side {
    if rng.range(2) == 0 {
        Side::Buy
    } else {
        Side::Sell
    }
}

fn shuffle(ids: &mut [u64], seed: u64) {
    let mut rng = Lcg(seed);
    for i in (1..ids.len()).rev() {
        let j = rng.range((i + 1) as u64) as usize;
        ids.swap(i, j);
    }
}

criterion_group!(
    benches,
    submit_limit_no_match,
    submit_market_full_sweep,
    cancel_random,
    cancel_same_price_stack,
    submit_into_deep_book,
    cancel_in_deep_book,
    mixed_workload_throughput,
);
criterion_main!(benches);
