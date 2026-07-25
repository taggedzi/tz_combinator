//! Black-box tests for the `zip` subcommand.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn zip_pairs_positionally() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--sep", "-"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\n");
}

#[test]
fn zip_default_policy_is_error() {
    let out = bin()
        .args(["zip", "--list", "a,b,c", "--list", "x,y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ZIP_LENGTH_MISMATCH"));
}

#[test]
fn zip_truncate_uses_shortest() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "a,b,c",
            "--list",
            "x,y",
            "--sep",
            "-",
            "--on-unequal",
            "truncate",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\n");
}

#[test]
fn zip_cycle_wraps_shorter_list() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "a,b,c",
            "--list",
            "x,y",
            "--sep",
            "-",
            "--on-unequal",
            "cycle",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\nc-x\n");
}

#[test]
fn zip_rejects_reverse_fields() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--reverse-fields"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn zip_count_only() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "a,b,c",
            "--list",
            "x,y",
            "--on-unequal",
            "truncate",
            "--count-only",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[test]
fn zip_jsonl_shape() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["fields"][0], "a");
    assert_eq!(first["fields"][1], "x");
}
