//! # Matcher - High-Performance Trading Engine
//!
//! Matcher is a next-generation order matching engine designed for ultra-low latency
//! and high-throughput trading applications.
//!
//! ## Features
//!
//! - Sub-microsecond matching latency
//! - Multi-product support
//! - Advanced order types (Market, Limit, IOC, FOK, Stop)
//! - Built-in risk management
//! - Comprehensive monitoring and metrics
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use matcher::{Engine, Config, Order, OrderType, Side};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = Config::from_file("config.toml")?;
//!     let mut engine = Engine::new(config).await?;
//!     
//!     // Start the engine
//!     engine.start().await?;
//!     
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod core;
pub mod network;
pub mod storage;
pub mod types;
pub mod utils;

// Re-export commonly used types
pub use config::Config;
pub use core::{Engine, MatchingEngine};
pub use types::{Order, OrderType, Side, MatchResult, ProductId};
pub use utils::{HighResTimer, Metrics};

/// Result type used throughout the matcher library
pub type Result<T> = std::result::Result<T, crate::types::MatcherError>;

/// Version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");