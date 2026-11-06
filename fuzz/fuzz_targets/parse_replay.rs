// ABOUTME: Fuzzes replay parsing end to end: the Gbx container, LZO body, chunk loop, ghosts, samples, and inputs.
// ABOUTME: Any panic, abort, or allocation past libFuzzer's limit is a bug; errors are the expected outcome.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = tmreplay::parse_replay(data);
});
