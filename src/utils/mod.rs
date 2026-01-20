//! Utility modules for the matcher engine
//! 
//! This module provides various utilities including:
//! - High-resolution timing
//! - Metrics collection and monitoring
//! - Performance optimization helpers

pub mod timer;
pub mod metrics;

pub use timer::{HighResTimer, current_timestamp_ns, current_timestamp_us, current_timestamp_ms};
pub use metrics::{Metrics, ResourceMonitor};