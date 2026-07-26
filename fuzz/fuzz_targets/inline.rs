#![no_main]
use combinator_codecs::{InputBudget, InputLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(4096)];
    if let Ok(value) = std::str::from_utf8(data) {
        let mut budget = InputBudget::new(4096, 64);
        let _ = combinator_codecs::input::split_escaped_inline(
            value,
            ",",
            InputLimits { max_input_bytes: 4096, max_item_bytes: 256, max_items_per_list: 64 },
            &mut budget,
        );
    }
});
