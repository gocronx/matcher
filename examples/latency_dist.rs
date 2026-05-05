//! One-shot per-op latency distribution probe for the matcher.
//!
//! Runs a deterministic 80% submit / 15% cancel / 5% market-sweep workload
//! and prints percentile breakdown. Use this when you want p99/p999 numbers
//! that criterion's median-focused output does not surface directly.
//!
//! Run with --release; debug builds are 10–100× slower and not representative:
//!
//!     cargo run --release --example latency_dist
//!
//! Per-op timing carries `Instant::now()` overhead (~20-40 ns on modern
//! hardware) which is included in every sample. For the very fastest ops
//! (~50 ns) this means the reported latency is dominated by measurement
//! overhead, not the matcher itself. Treat absolute numbers as upper
//! bounds; trust comparisons (before/after a change) more than absolutes.

use matcher::{Order, OrderBook, Side};
use std::time::Instant;

const STEPS: u32 = 200_000;

struct Lcg(u64);

impl Lcg {
    fn range(&mut self, upper: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0 % upper
    }
}

fn main() {
    let mut book = OrderBook::new();
    let mut samples: Vec<u128> = Vec::with_capacity(STEPS as usize);
    let mut rng = Lcg(0xDEADBEEF);
    let mut next_id: u64 = 1;
    let mut resting_ids: Vec<u64> = Vec::new();

    let total_start = Instant::now();

    for _ in 0..STEPS {
        let pick = rng.range(100);
        let start = Instant::now();

        if pick < 80 {
            let side = if rng.range(2) == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            let price = 100 + rng.range(21);
            let qty = 1 + rng.range(5);
            let id = next_id;
            next_id += 1;
            resting_ids.push(id);
            let _ = book.submit(Order::limit(id, side, price, qty), 0);
        } else if pick < 95 && !resting_ids.is_empty() {
            let idx = rng.range(resting_ids.len() as u64) as usize;
            let id = resting_ids.swap_remove(idx);
            let _ = book.cancel(id);
        } else {
            let side = if rng.range(2) == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            let qty = 1 + rng.range(50);
            let id = next_id;
            next_id += 1;
            let _ = book.submit(Order::market(id, side, qty), 0);
        }

        samples.push(start.elapsed().as_nanos());
    }

    let total = total_start.elapsed();
    samples.sort_unstable();

    let n = samples.len();
    let pct = |p: f64| samples[((n - 1) as f64 * p) as usize];

    println!("Mixed workload: 80% submit / 15% cancel / 5% market-sweep");
    println!("  N samples:    {n}");
    println!("  Total time:   {total:.2?}");
    println!(
        "  Throughput:   {:.0} ops/sec",
        n as f64 / total.as_secs_f64()
    );
    println!();
    println!("  Per-op latency (ns, includes Instant::now overhead):");
    println!("    min:  {}", samples[0]);
    println!("    p50:  {}", pct(0.50));
    println!("    p90:  {}", pct(0.90));
    println!("    p95:  {}", pct(0.95));
    println!("    p99:  {}", pct(0.99));
    println!("    p999: {}", pct(0.999));
    println!("    max:  {}", samples[n - 1]);
    println!();
    println!("  Resting orders at end: {}", book.len());
}
