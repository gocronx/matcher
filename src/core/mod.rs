//! Core matching engine components
//! 
//! This module contains the main matching engine logic including:
//! - Order book management
//! - Order matching algorithms
//! - Engine coordination and statistics

pub mod engine;
pub mod order_book;

pub use engine::{Engine, MatchingEngine};
pub use order_book::OrderBook;