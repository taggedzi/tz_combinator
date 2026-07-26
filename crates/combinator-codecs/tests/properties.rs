use combinator_codecs::{format_record_with, Format, InputBudget, InputLimits, Template};
use proptest::prelude::*;

fn limits() -> InputLimits {
    InputLimits {
        max_input_bytes: 4096,
        max_item_bytes: 256,
        max_items_per_list: 64,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, max_shrink_iters: 256, .. ProptestConfig::default() })]

    #[test]
    fn escaped_inline_input_never_panics(value in prop::collection::vec(any::<char>(), 0..128)) {
        let value: String = value.into_iter().filter(|c| *c != '\0').collect();
        let mut budget = InputBudget::new(4096, 64);
        let result = combinator_codecs::input::split_escaped_inline(&value, ",", limits(), &mut budget);
        prop_assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn templates_render_valid_utf8_bounded(literal in "[a-zA-Z0-9 _:;\\\\{}]{0,128}") {
        if let Ok(template) = Template::parse(&literal) {
            let fields = ["quote \"", "line\n", "雪"];
            let rendered = template.render(&fields, &[]);
            if let Ok(rendered) = rendered {
                prop_assert!(rendered.len() <= 2048);
            }
        }
    }

    #[test]
    fn serializers_produce_parseable_json(values in prop::collection::vec("[a-zA-Z0-9 ,\\\"\\n\\t]{0,32}", 0..4)) {
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        let line = format_record_with(&refs, 7, "|", "\n", Format::Jsonl, false, None, &[]).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        prop_assert!(parsed["value"].is_string());
        prop_assert!(parsed["fields"].is_array());
    }

    #[test]
    fn all_record_formats_remain_bounded(values in prop::collection::vec(".{0,24}", 0..4)) {
        let refs: Vec<&str> = values.iter().map(String::as_str).collect();
        for format in [Format::Text, Format::Jsonl, Format::Csv, Format::Tsv, Format::Nul] {
            let output = format_record_with(&refs, 0, "|", "\n", format, false, None, &[]).unwrap();
            prop_assert!(output.len() <= 4096);
        }
    }
}

#[test]
fn escaped_inline_consumes_aggregate_input_bytes() {
    let mut budget = InputBudget::new(20, 64);
    combinator_codecs::input::split_escaped_inline("aaaaaaaaaaaaaaa", ",", limits(), &mut budget)
        .unwrap();
    let error = combinator_codecs::input::split_escaped_inline(
        "bbbbbbbbbbbbbbb",
        ",",
        limits(),
        &mut budget,
    )
    .unwrap_err();
    assert_eq!(error.code, "INPUT_TOO_LARGE");
}
