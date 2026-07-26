#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(4096)];
    for line in data.split(|byte| *byte == b'\n').take(64) {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) {
            let _ = value.as_object().map(|object| object.values().all(|v| v.is_string()));
        }
    }
});
