#![no_main]

use libfuzzer_sys::fuzz_target;
use matcher::{Order, OrderBook, OrderType, Side, Price, Quantity, OrderId};
use arbitrary::Arbitrary;

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

fuzz_target!(|ops: Vec<BookOp>| {
    let mut book = OrderBook::new();
    
    for op in ops {
        match op {
            BookOp::Submit { id, side, price, qty, kind } => {
                if qty == 0 || id == 0 {
                    continue; // Skip invalid input
                }
                
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
                let _ = book.amend(id, new_price, new_qty);
            }
        }
        
        // Critical: check invariants after each operation
        // Expose assert_invariants for fuzzing via cfg(any(test, feature = "fuzzing"))
        #[cfg(any(test, feature = "fuzzing"))]
        book.assert_invariants();
    }
});
