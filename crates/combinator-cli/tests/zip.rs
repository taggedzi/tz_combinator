//! Black-box tests for the `zip` subcommand.

mod common;

use common::TempDir;
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
fn zip_reverse_walks_from_the_end() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "a,b",
            "--list",
            "x,y",
            "--sep",
            "-",
            "--reverse",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b-y\na-x\n");
}

#[test]
fn zip_runtime_output_limit_is_enforced() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "a,b",
            "--list",
            "x,y",
            "--sep",
            "-",
            "--max-output-bytes",
            "5",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_LIMIT_EXCEEDED"));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\n");
}

#[test]
fn zip_reads_files_and_writes_output_file() {
    let dir = TempDir::new("zip_files");
    let left = dir.join("left.txt");
    let right = dir.join("right.txt");
    let output = dir.join("output.txt");
    std::fs::write(&left, "a\nb\n").unwrap();
    std::fs::write(&right, "x\ny\n").unwrap();

    let out = bin()
        .args([
            "zip",
            "--file",
            left.to_str().unwrap(),
            "--file",
            right.to_str().unwrap(),
            "--sep",
            "-",
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "a-x\nb-y\n");
}

#[test]
fn zip_template_renders_selected_fields() {
    let out = bin()
        .args([
            "zip",
            "--list",
            "host1,host2",
            "--list",
            "80,443",
            "--template",
            "{0}:{1}",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "host1:80\nhost2:443\n"
    );
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
