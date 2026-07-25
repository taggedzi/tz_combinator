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

#[test]
fn concat_rejects_sep() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--sep", "-"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn concat_rejects_reverse_fields() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--reverse-fields"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
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
