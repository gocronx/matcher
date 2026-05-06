#![no_main]

use libfuzzer_sys::fuzz_target;
use matcher::OrderBook;

fuzz_target!(|data: &[u8]| {
    // Fuzz test snapshot loading - ensure no crash on malicious data
    let _ = OrderBook::load(data);
});
