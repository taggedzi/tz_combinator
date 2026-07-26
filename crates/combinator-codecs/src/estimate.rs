//! Output-size estimation over in-memory lists.

use combinator_core::Count;

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

fn combination_count(lens: &[usize]) -> Count {
    let mut total = 1u128;
    for &len in lens {
        total = match total.checked_mul(len as u128) {
            Some(value) => value,
            None => return Count::Overflow,
        };
    }
    Count::Exact(total)
}

fn item_bytes_across_combos(lists: &[Vec<String>], total: u128) -> Option<u128> {
    let mut acc = 0u128;
    for list in lists {
        let sum_len: u128 = list.iter().map(|s| s.len() as u128).sum();
        let contrib = sum_len.checked_mul(total / list.len() as u128)?;
        acc = acc.checked_add(contrib)?;
    }
    Some(acc)
}

pub fn estimate_text_size(input: &SizeInput) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(Vec::len).collect();
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
    let per_record_sep = match (input.field_sep_bytes as u128)
        .checked_mul(k.saturating_sub(1))
        .and_then(|v| v.checked_add(input.rec_sep_bytes as u128))
    {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    let sep_bytes = match total.checked_mul(per_record_sep) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    match item_bytes.checked_add(sep_bytes) {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

pub fn estimate_jsonl_size(input: &SizeInput, lean: bool) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(Vec::len).collect();
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
    let field_sep_in_value = match (input.field_sep_bytes as u128)
        .checked_mul(6)
        .and_then(|v| v.checked_mul(k.saturating_sub(1)))
        .and_then(|v| v.checked_mul(total))
    {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    let index_digits = decimal_width(total.saturating_sub(1)) as u128;
    let per_record = if lean {
        18 + index_digits
    } else {
        30 + index_digits
    };
    let escaped_item_bytes = match item_bytes.checked_mul(6) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    let mut variable = match escaped_item_bytes.checked_add(field_sep_in_value) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    if !lean {
        let two_k = match k.checked_mul(2) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        let quote_bytes = match total.checked_mul(two_k) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        let comma_bytes = match total.checked_mul(k.saturating_sub(1)) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        variable = match variable
            .checked_add(escaped_item_bytes)
            .and_then(|v| v.checked_add(quote_bytes))
            .and_then(|v| v.checked_add(comma_bytes))
        {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
    }
    match total
        .checked_mul(per_record)
        .and_then(|fixed| fixed.checked_add(variable))
    {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

fn decimal_width(mut n: u128) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut width = 0;
    while n > 0 {
        n /= 10;
        width += 1;
    }
    width
}
