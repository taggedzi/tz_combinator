#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(4096)];
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(template) = combinator_codecs::Template::parse(source) {
            let fields = ["a", "b", "c"];
            let _ = template.render(&fields, &[], 64 * 1024);
        }
    }
});
