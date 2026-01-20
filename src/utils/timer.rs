use std::time::{Duration, Instant};

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
use core::arch::x86_64::_rdtsc;

/// High-Resolution Timer for ultra-precise timing measurements
/// 
/// This timer provides nanosecond precision timing using:
/// - CPU timestamp counter (TSC) on x86/x86_64 architectures
/// - System monotonic clock on other architectures
/// 
/// # Performance Characteristics
/// - Zero allocation
/// - Lock-free operation
/// - Sub-nanosecond precision on modern CPUs
/// 
/// # Example
/// ```rust
/// use matcher::utils::HighResTimer;
/// 
/// let timer = HighResTimer::start();
/// // ... do some work ...
/// let elapsed_ns = timer.elapsed_ns();
/// println!("Operation took {} nanoseconds", elapsed_ns);
/// ```
pub struct HighResTimer {
    #[allow(dead_code)]
    start_cycles: u64,
    start_instant: Instant,
    #[allow(dead_code)]
    cpu_freq_ghz: f64,
}

impl HighResTimer {
    /// Start a new high-resolution timer
    /// 
    /// The CPU frequency is automatically detected or estimated.
    /// For maximum accuracy, you can provide the exact CPU frequency.
    pub fn start() -> Self {
        Self::start_with_freq(Self::estimate_cpu_freq())
    }

    /// Start timer with specific CPU frequency in GHz
    /// 
    /// # Arguments
    /// * `cpu_freq_ghz` - CPU base frequency in GHz (e.g., 3.5 for 3.5 GHz)
    pub fn start_with_freq(cpu_freq_ghz: f64) -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let start_cycles = unsafe { _rdtsc() };
        
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        let start_cycles = 0;

        Self {
            start_cycles,
            start_instant: Instant::now(),
            cpu_freq_ghz,
        }
    }

    /// Get elapsed time in nanoseconds
    pub fn elapsed_ns(&self) -> u64 {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let end_cycles = unsafe { _rdtsc() };
            let delta_cycles = end_cycles.saturating_sub(self.start_cycles);
            
            // Convert cycles to nanoseconds
            // Formula: nanoseconds = cycles / (frequency_ghz * 1e9) * 1e9 = cycles / frequency_ghz
            (delta_cycles as f64 / self.cpu_freq_ghz) as u64
        }

        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            self.start_instant.elapsed().as_nanos() as u64
        }
    }

    /// Get elapsed time in microseconds
    pub fn elapsed_us(&self) -> f64 {
        self.elapsed_ns() as f64 / 1_000.0
    }

    /// Get elapsed time in milliseconds
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed_ns() as f64 / 1_000_000.0
    }

    /// Get elapsed time as Duration
    pub fn elapsed(&self) -> Duration {
        Duration::from_nanos(self.elapsed_ns())
    }

    /// Estimate CPU frequency in GHz
    /// 
    /// This provides a rough estimate. For production use, consider:
    /// - Reading from /proc/cpuinfo on Linux
    /// - Using sysctl on macOS
    /// - Using WMI on Windows
    fn estimate_cpu_freq() -> f64 {
        // Simple calibration method
        let start = Instant::now();
        
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        let start_cycles = unsafe { _rdtsc() };
        
        // Busy wait for approximately 1ms
        let target_duration = Duration::from_millis(1);
        while start.elapsed() < target_duration {
            std::hint::spin_loop();
        }
        
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let end_cycles = unsafe { _rdtsc() };
            let elapsed_cycles = end_cycles.saturating_sub(start_cycles);
            let elapsed_seconds = start.elapsed().as_secs_f64();
            
            if elapsed_seconds > 0.0 {
                // Convert to GHz
                (elapsed_cycles as f64) / elapsed_seconds / 1e9
            } else {
                3.0 // Default fallback
            }
        }
        
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        {
            3.0 // Default fallback for non-x86 architectures
        }
    }
}

/// Utility function to get current timestamp in nanoseconds since Unix epoch
pub fn current_timestamp_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

/// Utility function to get current timestamp in microseconds since Unix epoch
pub fn current_timestamp_us() -> u64 {
    current_timestamp_ns() / 1_000
}

/// Utility function to get current timestamp in milliseconds since Unix epoch
pub fn current_timestamp_ms() -> u64 {
    current_timestamp_ns() / 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_timer_basic_functionality() {
        let timer = HighResTimer::start();
        thread::sleep(Duration::from_millis(1));
        let elapsed = timer.elapsed_ns();
        
        // Should be at least 1ms (1,000,000 ns) but allow for some variance
        assert!(elapsed >= 500_000, "Timer should measure at least 0.5ms");
        assert!(elapsed < 10_000_000, "Timer should not measure more than 10ms for 1ms sleep");
    }

    #[test]
    fn test_timer_precision() {
        let timer = HighResTimer::start();
        // Very short operation
        let _sum: u64 = (0..1000).sum();
        let elapsed = timer.elapsed_ns();
        
        // Should be measurable but very small
        assert!(elapsed > 0, "Timer should measure non-zero time");
        assert!(elapsed < 1_000_000, "Simple operation should take less than 1ms");
    }

    #[test]
    fn test_timestamp_functions() {
        let ts_ns = current_timestamp_ns();
        let ts_us = current_timestamp_us();
        let ts_ms = current_timestamp_ms();
        
        assert!(ts_ns > ts_us);
        assert!(ts_us > ts_ms);
        
        // Check rough conversion
        assert!((ts_ns / 1_000).abs_diff(ts_us) < 1000);
        assert!((ts_us / 1_000).abs_diff(ts_ms) < 1000);
    }
}