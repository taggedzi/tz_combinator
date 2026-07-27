//! Black-box coverage for the new selection operations and typed filters.

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
        .args(args)
        .output()
        .expect("run combinator")
}

#[test]
fn permutations_emit_deterministic_order_and_page() {
    let output = run(&[
        "permutations",
        "--list",
        "a,b,c",
        "--offset",
        "1",
        "--limit",
        "2",
    ]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "acb\nbac\n");
}

#[test]
fn combinations_and_variations_have_distinct_shapes() {
    let combinations = run(&["combinations", "--list", "a,b,c", "--choose", "2"]);
    assert!(combinations.status.success());
    assert_eq!(
        String::from_utf8_lossy(&combinations.stdout),
        "ab\nac\nbc\n"
    );

    let variations = run(&["variations", "--list", "a,b,c", "--length", "2"]);
    assert!(variations.status.success());
    assert_eq!(
        String::from_utf8_lossy(&variations.stdout),
        "ab\nac\nba\nbc\nca\ncb\n"
    );
}

#[test]
fn filters_are_typed_and_repeated_filters_are_conjunctive() {
    let output = run(&[
        "permutations",
        "--list",
        "aa,ab,ba",
        "--filter",
        "prefix:0=a",
        "--filter",
        "length:0=2..2",
    ]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "aaabba\naabaab\nabaaba\nabbaaa\n"
    );
}

#[test]
fn not_equal_filter_excludes_matching_field_values() {
    let output = run(&["product", "--list", "red,blue", "--filter", "neq:0=red"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "blue\n");
}

#[test]
fn malformed_filters_and_filtered_count_are_usage_errors() {
    let malformed = run(&["permutations", "--list", "a,b", "--filter", "unknown:0=x"]);
    assert_eq!(malformed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("FILTER_INVALID"));

    let count = run(&[
        "permutations",
        "--list",
        "a,b",
        "--filter",
        "prefix:0=a",
        "--count-only",
    ]);
    assert_eq!(count.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&count.stderr).contains("FILTER_MODE_UNSUPPORTED"));
}
