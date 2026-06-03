//! Order amendment example: modify price and quantity of resting orders.
//!
//! Run with:
//!     cargo run --example amend

use matcher::{BookEvent, Order, OrderBook, Price, Quantity, Side};

fn main() {
    let mut book = OrderBook::new();

    println!("=== Order Amendment Demo ===\n");

    // Setup: two sell orders at the same price
    println!("1. Submit two sell orders at price 100:");
    book.submit(Order::limit(1, Side::Sell, 100, 10), 0);
    book.submit(Order::limit(2, Side::Sell, 100, 5), 1);
    println!("   Order 1: sell 10 @ 100 (older)");
    println!("   Order 2: sell 5 @ 100 (newer)");
    println!("   Total at 100: {}\n", book.level_qty(Side::Sell, 100));

    // Quantity decrease maintains time priority
    println!("2. Amend order 1: reduce quantity from 10 to 3");
    let events = book.amend(1u64, None, Some(Quantity(3)));
    print_events(&events);
    println!("   Total at 100: {}\n", book.level_qty(Side::Sell, 100));

    // Verify time priority maintained
    println!("3. Market buy 3 units (should match order 1 first):");
    let trades = book.submit(Order::market(3, Side::Buy, 3), 2);
    println!("   Matched with order {}", trades[0].sell_id);
    println!("   Remaining: order 2 still has 5 units\n");

    // Price change loses time priority
    println!("4. Submit order 4 at price 101:");
    book.submit(Order::limit(4, Side::Sell, 101, 8), 3);
    println!("   Order 4: sell 8 @ 101\n");

    println!("5. Amend order 2: change price from 100 to 101");
    let events = book.amend(2u64, Some(Price(101)), None);
    print_events(&events);
    println!("   Order 2 moved to back of queue at 101\n");

    // Verify time priority lost
    println!("6. Market buy 5 units at 101 (should match order 4 first):");
    let trades = book.submit(Order::market(5, Side::Buy, 5), 4);
    println!("   Matched with order {}", trades[0].sell_id);
    println!("   Order 4 now has 3 units remaining\n");

    // Iceberg amendment
    println!("7. Submit iceberg order: total 30, visible 10:");
    book.submit(Order::iceberg(6, Side::Sell, 102, 30, 10), 5);
    println!("   Visible at 102: {}\n", book.level_qty(Side::Sell, 102));

    println!("8. Amend iceberg: reduce total from 30 to 15");
    let events = book.amend(6u64, None, Some(Quantity(15)));
    print_events(&events);
    println!(
        "   Visible at 102: {} (unchanged)",
        book.level_qty(Side::Sell, 102)
    );
    println!("   Hidden reduced from 20 to 5\n");

    // Rejection examples
    println!("9. Try to increase quantity (rejected):");
    let events = book.amend(6u64, None, Some(Quantity(20)));
    print_events(&events);

    println!("\n10. Try to amend unknown order (rejected):");
    let events = book.amend(999u64, Some(Price(100)), None);
    print_events(&events);

    println!("\n=== Final book state ===");
    println!("Best ask: {:?}", book.best_ask());
    println!("Total orders: {}", book.len());
}

fn print_events(events: &[BookEvent]) {
    for e in events {
        match e {
            BookEvent::Amended {
                order_id,
                new_price,
                new_quantity,
            } => {
                if let Some(price) = new_price {
                    println!(
                        "   ✓ Amended order {}: new price={}, qty={}",
                        order_id, price, new_quantity
                    );
                } else {
                    println!("   ✓ Amended order {}: qty={}", order_id, new_quantity);
                }
            }
            BookEvent::AmendRejected { order_id, reason } => {
                println!(
                    "   ✗ Amendment rejected for order {}: {:?}",
                    order_id, reason
                );
            }
            _ => {}
        }
    }
}
