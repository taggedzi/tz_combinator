//! Stable black-box CLI contract tests.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread;

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

/// Drain both child pipes independently. This is intentionally not based on
/// `wait_with_output`, so diagnostics cannot deadlock a producer with a busy
/// stdout pipe.
fn run(args: &[&str]) -> Output {
    let mut child = command()
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn combinator");
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stdout;
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut reader = stderr;
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let status = child.wait().expect("wait combinator");
    Output {
        status,
        stdout: out_reader.join().unwrap(),
        stderr: err_reader.join().unwrap(),
    }
}

#[test]
fn text_product_zip_and_concat_are_byte_stable() {
    let product = run(&["product", "--list", "a,b", "--list", "1,2", "--sep", "-"]);
    assert!(product.status.success());
    assert_eq!(product.stdout, include_bytes!("golden/product.text"));
    assert!(product.stderr.is_empty());

    let zip = run(&["zip", "--list", "a,b", "--list", "1,2", "--sep", "-"]);
    assert!(zip.status.success());
    assert_eq!(zip.stdout, include_bytes!("golden/zip.text"));

    let concat = run(&["concat", "--list", "a,b", "--list", "1,2"]);
    assert!(concat.status.success());
    assert_eq!(concat.stdout, include_bytes!("golden/concat.text"));
}

#[test]
fn jsonl_full_and_lean_records_have_the_documented_shape() {
    let full = run(&["--list", "a,b", "--list", "1", "--format", "jsonl"]);
    assert!(full.status.success());
    for (index, line) in full
        .stdout
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .enumerate()
    {
        let value: serde_json::Value = serde_json::from_slice(line).unwrap();
        assert_eq!(value["i"], index);
        assert!(value["value"].is_string());
        assert!(value["fields"].is_array());
        assert!(value.get("named").is_none());
    }

    let lean = run(&[
        "--list",
        "a,b",
        "--list",
        "1",
        "--format",
        "jsonl",
        "--lean-output",
    ]);
    assert!(lean.status.success());
    assert_eq!(lean.stdout, b"\"a1\"\n\"b1\"\n");
}

#[test]
fn explain_json_is_versioned_and_typed() {
    let out = run(&[
        "--list",
        "a,b",
        "--list",
        "1,2",
        "--explain",
        "--format",
        "json",
    ]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for key in [
        "schema_version",
        "operation",
        "combination_count",
        "offset",
        "limit",
        "records_to_emit",
        "estimated_output_bytes",
        "output",
        "format",
        "limits",
    ] {
        assert!(value.get(key).is_some(), "missing {key}");
    }
    assert!(value["schema_version"].is_u64());
    assert!(value["operation"].is_string());
    assert!(value["combination_count"].is_u64());
    assert!(value["limits"].is_object());
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["operation"], "product");
}

#[test]
fn diagnostics_and_exit_codes_never_leak_to_stdout() {
    let usage = run(&[]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());
    assert_eq!(usage.stderr, include_bytes!("golden/no-lists.stderr"));

    let runtime = run(&["--list", "a,b", "--max-output-bytes", "3"]);
    assert_eq!(runtime.status.code(), Some(1));
    assert!(!runtime.stderr.is_empty());
    assert!(!runtime.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&runtime.stderr).contains("a\n"));
}

#[test]
fn jsonl_errors_are_json_and_large_dual_pipe_runs_complete() {
    let error = run(&["--format", "jsonl"]);
    assert_eq!(error.status.code(), Some(2));
    let diagnostic: serde_json::Value = serde_json::from_slice(&error.stderr).unwrap();
    assert_eq!(diagnostic["error"]["code"], "NO_LISTS");
    assert!(error.stdout.is_empty());

    // Keep the argument below Windows' command-line length limit while still
    // producing enough output to exercise pipe draining.
    let list = (0..8_000).map(|_| "x").collect::<Vec<_>>().join(",");
    let args = ["--list", list.as_str(), "--summary"];
    let large = run(&args);
    assert!(large.status.success());
    assert_eq!(large.stdout.iter().filter(|b| **b == b'\n').count(), 8_000);
    assert!(String::from_utf8_lossy(&large.stderr).contains("summary[OUTPUT]"));
}

#[test]
fn version_output_matches_package_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("combinator {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(out.stderr.is_empty());
}
