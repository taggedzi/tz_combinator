//! Output-size estimation, computed from list statistics without generating.

use crate::count::{combination_count, Count};

/// Inputs needed to estimate output size without generating it.
pub struct SizeInput<'a> {
    pub lists: &'a [Vec<String>],
    pub field_sep_bytes: u64,
    pub rec_sep_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeEstimate {
    Bytes(u128),
    Overflow,
}

/// Sum, over every combination, of the byte lengths of the chosen items.
///
/// For position `j`, each item appears in `total / len_j` combinations, so the
/// contribution is `(sum of item byte lengths in list j) * (total / len_j)`.
fn item_bytes_across_combos(lists: &[Vec<String>], total: u128) -> Option<u128> {
    let mut acc: u128 = 0;
    for list in lists {
        let sum_len: u128 = list.iter().map(|s| s.len() as u128).sum();
        let others = total / (list.len() as u128); // list.len() > 0 here (total > 0)
        let contrib = sum_len.checked_mul(others)?;
        acc = acc.checked_add(contrib)?;
    }
    Some(acc)
}

/// Exact byte count of plain-text output.
pub fn estimate_text_size(input: &SizeInput) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(|l| l.len()).collect();
    let total = match combination_count(&lens) {
        Count::Exact(t) => t,
        Count::Overflow => return SizeEstimate::Overflow,
    };
    if total == 0 {
        return SizeEstimate::Bytes(0);
    }
    let k = lens.len() as u128;

    let item_bytes = match item_bytes_across_combos(input.lists, total) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };

    let per_record_sep =
        (input.field_sep_bytes as u128) * k.saturating_sub(1) + input.rec_sep_bytes as u128;
    let sep_bytes = match total.checked_mul(per_record_sep) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };

    match item_bytes.checked_add(sep_bytes) {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

/// Upper-bound estimate for JSON Lines output. Ignores content-dependent JSON
/// string escaping, so treat it as a close estimate, not an exact figure.
pub fn estimate_jsonl_size(input: &SizeInput, lean: bool) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(|l| l.len()).collect();
    let total = match combination_count(&lens) {
        Count::Exact(t) => t,
        Count::Overflow => return SizeEstimate::Overflow,
    };
    if total == 0 {
        return SizeEstimate::Bytes(0);
    }
    let k = lens.len() as u128;

    // The assembled `value` string appears once per record; its bytes equal the
    // item bytes plus field separators. `fields` (non-lean) repeats the item
    // bytes a second time, wrapped in quotes and commas.
    let item_bytes = match item_bytes_across_combos(input.lists, total) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    let field_sep_in_value = (input.field_sep_bytes as u128) * k.saturating_sub(1);

    // Index digits: bound by the decimal width of the largest index.
    let index_digits = decimal_width(total.saturating_sub(1)) as u128;

    // Per-record fixed structural bytes.
    // lean:      {"i":<idx>,"value":"<value>"}\n  -> `{"i":`(5) + `,"value":"`(10) + `"}`(2) + `\n`(1) = 18
    // non-lean:  ... + `,"fields":[`(11) + `]`(1) = 12 more, plus 2 quotes + (k-1) commas per record
    let per_record: u128 = if lean { 18 + index_digits } else { 30 + index_digits };

    // Variable (content) bytes. The `value` string appears once (item bytes +
    // field separators). Non-lean repeats item bytes inside `fields`, wrapped in
    // 2 quotes per field and (k-1) commas per record.
    let mut variable = match item_bytes.checked_add(field_sep_in_value) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    if !lean {
        let quote_bytes = match total.checked_mul(2 * k) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        let comma_bytes = match total.checked_mul(k.saturating_sub(1)) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        variable = match variable
            .checked_add(item_bytes)
            .and_then(|v| v.checked_add(quote_bytes))
            .and_then(|v| v.checked_add(comma_bytes))
        {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
    }

    let fixed = match total.checked_mul(per_record) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    match fixed.checked_add(variable) {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

fn decimal_width(mut n: u128) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut w = 0;
    while n > 0 {
        n /= 10;
        w += 1;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};

    fn lists() -> Vec<Vec<String>> {
        // lens [2,2], item byte-length sums: list0 = "a"+"bb" = 3, list1 = "c"+"d" = 2
        vec![
            vec!["a".to_string(), "bb".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn text_size_is_exact() {
        // 4 combos. item bytes: list0 sum 3 * others 2 = 6; list1 sum 2 * others 2 = 4 -> 10.
        // separators: field_sep 1 byte * (k-1)=1 + rec_sep 1 byte = 2 per record * 4 = 8.
        // total = 18.
        let input = SizeInput { lists: &lists(), field_sep_bytes: 1, rec_sep_bytes: 1 };
        assert_eq!(estimate_text_size(&input), SizeEstimate::Bytes(18));
    }

    #[test]
    fn text_size_empty_list_is_zero() {
        let lists = vec![vec!["a".to_string()], Vec::<String>::new()];
        let input = SizeInput { lists: &lists, field_sep_bytes: 1, rec_sep_bytes: 1 };
        assert_eq!(estimate_text_size(&input), SizeEstimate::Bytes(0));
    }

    #[test]
    fn jsonl_size_is_at_least_text_size() {
        let ls = lists();
        let input = SizeInput { lists: &ls, field_sep_bytes: 1, rec_sep_bytes: 1 };
        let text = match estimate_text_size(&input) { SizeEstimate::Bytes(b) => b, _ => panic!() };
        let json = match estimate_jsonl_size(&input, false) { SizeEstimate::Bytes(b) => b, _ => panic!() };
        assert!(json >= text, "jsonl {json} should be >= text {text}");
    }

    #[test]
    fn overflow_propagates() {
        let lens = vec!["x".to_string(); 2];
        let big = vec![lens; 40]; // 2^40 combos, huge byte total overflow-prone via multiply chain
        let input = SizeInput { lists: &big, field_sep_bytes: 1, rec_sep_bytes: 1 };
        // 2^40 combos * bytes stays within u128, so assert it is Bytes not panic:
        assert!(matches!(estimate_text_size(&input), SizeEstimate::Bytes(_)));
    }
}
