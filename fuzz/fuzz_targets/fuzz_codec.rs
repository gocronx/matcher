#![no_main]

use libfuzzer_sys::fuzz_target;
use matcher::codec::decode_inbound;

fuzz_target!(|data: &[u8]| {
    // Fuzz test decoder - ensure no panic on any input
    let _ = decode_inbound(data);
});
