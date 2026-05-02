//! Order-matching engine library.
//!
//! The headline API is [`OrderBook`]: feed it [`Order`]s, get [`Trade`]s back.
//! Everything else (`codec`, `matcher`, `net`) is optional plumbing for one
//! specific deployment shape — a UDP-multicast daemon, implemented in
//! `src/bin/matcher.rs`. Pick what you want; ignore the rest.
//!
//! ```
//! use matcher::{Order, OrderBook, OrderType, Side};
//!
//! let mut book = OrderBook::new();
//! book.submit(Order { id: 1, side: Side::Sell, kind: OrderType::Limit,
//!                     price: 100, quantity: 5, filled: 0, hidden: 0 }, 0);
//!
//! let trades = book.submit(Order { id: 2, side: Side::Buy, kind: OrderType::Limit,
//!                     price: 100, quantity: 3, filled: 0, hidden: 0 }, 1);
//! assert_eq!(trades.len(), 1);
//! assert_eq!(trades[0].quantity, 3);
//! ```

pub mod book;
pub mod codec;
pub mod matcher;
pub mod net;
pub mod types;

pub use book::OrderBook;
pub use types::{Order, OrderType, Side, Trade};
