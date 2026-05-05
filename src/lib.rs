//! Order-matching engine library.
//!
//! The headline API is [`OrderBook`]: feed it [`Order`]s, get [`Trade`]s back,
//! or call [`OrderBook::submit_events`] / [`OrderBook::cancel_events`] when
//! callers need accepts, rejects, cancels, and resting-state updates too.
//! Everything else (`codec`, `matcher`, `net`) is optional plumbing for one
//! specific deployment shape — a UDP-multicast daemon, implemented in
//! `src/bin/matcher.rs`. Pick what you want; ignore the rest.
//!
//! ```
//! use matcher::{Order, OrderBook, Side};
//!
//! let mut book = OrderBook::new();
//! book.submit(Order::limit(1, Side::Sell, 100, 5), 0);
//!
//! let trades = book.submit(Order::limit(2, Side::Buy, 100, 3), 1);
//! assert_eq!(trades.len(), 1);
//! assert_eq!(trades[0].quantity.get(), 3);
//! ```

pub mod book;
pub mod codec;
pub mod matcher;
pub mod net;
pub mod types;

pub use book::OrderBook;
pub use types::{
    BookEvent, CancelRejectReason, Order, OrderId, OrderType, Price, Quantity, RejectReason, Side,
    Timestamp, Trade,
};
