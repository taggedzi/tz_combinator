//! Conservative classification of field prefixes that downstream tabular
//! consumers may interpret as formulas or active expressions.
//!
//! This module does not sanitize or rewrite data. Version 1 inspects only the
//! first Unicode scalar value. It deliberately does not trim whitespace or
//! normalize Unicode because either operation would change the classification
//! contract and could diverge from the bytes a downstream consumer receives.

/// Version of the documented formula-like prefix classification contract.
pub const FORMULA_PREFIX_POLICY_VERSION: u32 = 1;

/// Returns whether a field begins with a version-1 formula-like prefix.
///
/// The version-1 set is the ASCII formula initiators `=`, `+`, `-`, and `@`;
/// leading horizontal tab, carriage return, or line feed; and the full-width
/// variants `＝`, `＋`, `－`, and `＠`. A matching character later in a field
/// does not classify the field. The result identifies a downstream
/// interpretation risk; it does not prove that a particular consumer will
/// execute the field.
pub fn is_formula_like_field(value: &str) -> bool {
    matches!(
        value.chars().next(),
        Some(
            '=' | '+'
                | '-'
                | '@'
                | '\t'
                | '\r'
                | '\n'
                | '\u{ff1d}'
                | '\u{ff0b}'
                | '\u{ff0d}'
                | '\u{ff20}'
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_one_prefix_set_is_explicit() {
        assert_eq!(FORMULA_PREFIX_POLICY_VERSION, 1);
        for value in [
            "=2+3",
            "+2",
            "-2",
            "@example",
            "\tvalue",
            "\rvalue",
            "\nvalue",
            "＝2+3",
            "＋2",
            "－2",
            "＠example",
        ] {
            assert!(is_formula_like_field(value), "missed {value:?}");
        }
    }

    #[test]
    fn only_the_first_scalar_is_classified_without_normalization() {
        for value in [
            "", "plain", "42", " =2+3", "x=2+3", "x\t=2+3", "é=2+3", "﹦2+3",
        ] {
            assert!(!is_formula_like_field(value), "overmatched {value:?}");
        }
    }
}
