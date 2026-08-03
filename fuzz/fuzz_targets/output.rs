#![no_main]
use combinator_codecs::{format_record_with, Format};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(4096)];
    let fields: Vec<String> = data
        .split(|byte| *byte == 0)
        .take(64)
        .map(|part| String::from_utf8_lossy(&part[..part.len().min(256)]).into_owned())
        .collect();
    let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
    for format in [Format::Text, Format::Jsonl, Format::Csv, Format::Tsv, Format::Nul] {
        let _ = format_record_with(
            &refs,
            0,
            "|",
            "\n",
            format,
            false,
            None,
            &[],
            64 * 1024,
        );
    }
});
