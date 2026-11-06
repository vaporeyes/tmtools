// ABOUTME: Fuzzes the exported mesh decoder: the header, deflate body, node tree, visuals, surfaces, and lights.
// ABOUTME: Any panic, abort, or allocation past libFuzzer's limit is a bug; errors are the expected outcome.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = tmassets::solid::decode_solid(data);
});
