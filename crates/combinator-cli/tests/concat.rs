//! Black-box tests for the `concat` subcommand.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn concat_emits_every_list_in_order() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y,z"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\nx\ny\nz\n");
}

// Exit 2 alone does not identify these: clap parse errors and the CLI's own
// usage errors share it, so a typo in the arguments below would still pass.
// Each rejection test names the flag it expects to be refused.

#[test]
fn concat_rejects_sep() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--sep", "-"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unexpected argument"), "stderr: {stderr}");
    assert!(stderr.contains("--sep"), "stderr: {stderr}");
}

#[test]
fn concat_rejects_reverse_fields() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--reverse-fields"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unexpected argument"), "stderr: {stderr}");
    assert!(stderr.contains("--reverse-fields"), "stderr: {stderr}");
}

#[test]
fn concat_rejects_on_unequal() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--on-unequal", "truncate"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unexpected argument"), "stderr: {stderr}");
    assert!(stderr.contains("--on-unequal"), "stderr: {stderr}");
}

#[test]
fn concat_count_only() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y,z", "--count-only"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5");
}

#[test]
fn concat_offset_and_limit_paginate() {
    let out = bin()
        .args([
            "concat", "--list", "a,b", "--list", "x,y,z", "--offset", "1", "--limit", "2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b\nx\n");
}

#[test]
fn concat_runtime_output_limit_is_enforced() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--max-output-bytes", "2"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_LIMIT_EXCEEDED"));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\n");
}

#[test]
fn concat_reads_files_and_writes_output_file() {
    let dir = std::env::temp_dir();
    let stem = format!("combinator_concat_files_{}", std::process::id());
    let first = dir.join(format!("{stem}_first.txt"));
    let second = dir.join(format!("{stem}_second.txt"));
    let output = dir.join(format!("{stem}_output.txt"));
    std::fs::write(&first, "a\nb\n").unwrap();
    std::fs::write(&second, "x\ny\n").unwrap();

    let out = bin()
        .args([
            "concat",
            "--file",
            first.to_str().unwrap(),
            "--file",
            second.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let contents = std::fs::read_to_string(&output).unwrap_or_default();
    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
    std::fs::remove_file(&output).ok();

    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(contents, "a\nb\nx\ny\n");
}

#[test]
fn concat_template_renders_its_single_field() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--template", "value={0}"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "value=a\nvalue=b\n");
}

/// A template position beyond the input-list count is rejected at validation
/// time. Note this is validated against the number of lists, not against
/// concat's actual single-field record arity.
#[test]
fn concat_template_rejects_an_out_of_range_field() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--template", "{5}"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("TEMPLATE_UNKNOWN_FIELD"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("position=5"), "stderr: {stderr}");
}

/// Concat's per-record size bound is the longest item across *all* lists
/// rendered as a single field, not one item per list joined together.
///
/// Three lists of four items give 12 concat records; the widest item is
/// `bbbb`, so the bound is 12 * len("bbbb\n") = 60. Were concat to use the
/// product bound it would join one item per list ("abbbbc\n"), giving
/// 12 * 7 = 84. The list count is high enough that this bound, rather than
/// the codec's product-shaped estimate, is the reported minimum.
#[test]
fn concat_size_bound_uses_the_widest_item_as_a_single_field() {
    let out = bin()
        .args([
            "concat",
            "--list",
            "a,a,a,a",
            "--list",
            "bbbb,bbbb,bbbb,bbbb",
            "--list",
            "c,c,c,c",
            "--explain",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Match whole lines: `contains` would also accept 120 and 600.
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines.contains(&"combination_count=12"), "stdout: {stdout}");
    assert!(
        lines.contains(&"estimated_output_bytes=60"),
        "stdout: {stdout}"
    );
}

#[test]
fn concat_jsonl_shape_has_single_element_fields() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["value"], "a");
    assert_eq!(first["fields"].as_array().unwrap().len(), 1);
    assert_eq!(first["fields"][0], "a");
}

#[test]
fn concat_reverse_walks_from_the_end() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y", "--reverse"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "y\nx\nb\na\n");
}
