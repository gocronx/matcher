#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use matcher::{Order, OrderBook, Price, Quantity, Side};

#[derive(Arbitrary, Debug)]
enum BookOp {
    Submit {
        id: u64,
        side: bool, // true = Buy, false = Sell
        price: u64,
        qty: u64,
        kind: u8, // 0=Limit, 1=Market, 2=IOC, 3=FOK, 4=PostOnly, 5=Iceberg
    },
    Cancel {
        id: u64,
    },
    Amend {
        id: u64,
        new_price: Option<u64>,
        new_qty: Option<u64>,
    },
}

// Keep fuzzed prices/quantities within the library's capacity contract
// (Quantity arithmetic is fail-fast on overflow). A level holds at most a few
// thousand fuzzed orders, so 1e9 per order keeps sums far below u64::MAX while
// still exercising large-value paths. We fuzz LOGIC here, not the documented
// overflow panic.
const MAX_PRICE: u64 = 1_000_000;
const MAX_QTY: u64 = 1_000_000_000;

fuzz_target!(|ops: Vec<BookOp>| {
    let mut book = OrderBook::new();

    for op in ops {
        match op {
            BookOp::Submit { id, side, price, qty, kind } => {
                if qty == 0 || id == 0 {
                    continue; // Skip invalid input
                }
                let price = price % MAX_PRICE + 1;
                let qty = qty % MAX_QTY + 1;

                let side = if side { Side::Buy } else { Side::Sell };
                let order = match kind % 6 {
                    0 => Order::limit(id, side, price, qty),
                    1 => Order::market(id, side, qty),
                    2 => Order::ioc(id, side, price, qty),
                    3 => Order::fok(id, side, price, qty),
                    4 => Order::post_only(id, side, price, qty),
                    _ => {
                        let visible = qty / 2 + 1;
                        Order::iceberg(id, side, price, qty, visible)
                    }
                };

                let _ = book.submit(order, 0);
            }
            BookOp::Cancel { id } => {
                let _ = book.cancel(id);
            }
            BookOp::Amend { id, new_price, new_qty } => {
                let _ = book.amend(
                    id,
                    new_price.map(|p| Price::from(p % MAX_PRICE + 1)),
                    new_qty.map(|q| Quantity::from(q % MAX_QTY + 1)),
                );
            }
        }

        // Critical: check invariants after each operation. This crate always
        // builds matcher with the "fuzzing" feature (see fuzz/Cargo.toml), so
        // the call is unconditional — a cfg(feature) here would test THIS
        // crate's features (never set) and silently compile the check away.
        book.assert_invariants();
    }
});
