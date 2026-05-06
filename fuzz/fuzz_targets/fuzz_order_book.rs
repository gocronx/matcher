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
        kind: u8, // 0=Limit, 1=Market, 2=IOC, 3=FOK, 4=PostOnly
    },
    Cancel {
        id: u64,
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
                let order = match kind % 5 {
                    0 => Order::limit(id, side, price, qty),
                    1 => Order::market(id, side, qty),
                    2 => Order::ioc(id, side, price, qty),
                    3 => Order::fok(id, side, price, qty),
                    _ => Order::post_only(id, side, price, qty),
                };
                
                let _ = book.submit(order, 0);
            }
            BookOp::Cancel { id } => {
                let _ = book.cancel(id);
            }
        }
        
        // Critical: check invariants after each operation
        #[cfg(test)]
        book.assert_invariants();
    }
});
