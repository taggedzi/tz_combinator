//! Stable black-box CLI contract tests.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread;

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_combinator"));
    command.env_remove("COMBINATOR_LOG");
    command
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

fn run_with_log_env(args: &[&str], value: &str) -> Output {
    let mut child = command()
        .env("COMBINATOR_LOG", value)
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
    let usage = run(&["product"]);
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
fn opt_in_text_logs_are_stderr_only_and_phase_bounded() {
    let out = run(&[
        "--list",
        "a,b",
        "--list",
        "1,2",
        "--sep",
        "-",
        "--log-level",
        "debug",
    ]);
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a-1\na-2\nb-1\nb-2\n");
    let stderr = String::from_utf8(out.stderr).unwrap();
    let lines: Vec<_> = stderr.lines().collect();
    assert!(lines.iter().any(|line| line.contains("invocation_started")));
    assert!(lines.iter().any(|line| line.contains("input_complete")));
    assert!(lines
        .iter()
        .any(|line| line.contains("generation_complete")));
    assert!(lines.iter().all(|line| !line.contains("a-1")));
    assert!(lines.len() <= 6);
}

#[test]
fn opt_in_json_logs_and_diagnostics_are_independent_json_lines() {
    let success = run(&[
        "--list",
        "a,b",
        "--format",
        "jsonl",
        "--log-level",
        "debug",
        "--log-format",
        "json",
    ]);
    assert!(success.status.success());
    assert_eq!(success.stdout, b"{\"i\":0,\"value\":\"a\",\"fields\":[\"a\"]}\n{\"i\":1,\"value\":\"b\",\"fields\":[\"b\"]}\n");
    let success_lines: Vec<serde_json::Value> = String::from_utf8(success.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(success_lines.iter().all(|line| line["kind"] == "log"));
    assert!(success_lines
        .iter()
        .any(|line| line["event"] == "input_complete"));
    assert!(success_lines
        .iter()
        .any(|line| line["event"] == "generation_complete"));

    let failure = run(&[
        "--format",
        "jsonl",
        "--log-level",
        "debug",
        "--log-format",
        "json",
    ]);
    assert_eq!(failure.status.code(), Some(2));
    let failure_lines: Vec<serde_json::Value> = String::from_utf8(failure.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(failure_lines
        .iter()
        .any(|line| line["kind"] == "diagnostic"));
    assert!(failure_lines.iter().all(|line| line.get("kind").is_some()));

    let warning_path = std::env::temp_dir().join(format!(
        "combinator-logging-empty-{}.txt",
        std::process::id()
    ));
    std::fs::write(&warning_path, b"").unwrap();
    let warning_path = warning_path.to_string_lossy().into_owned();
    let warning_summary = run(&[
        "--file",
        warning_path.as_str(),
        "--format",
        "jsonl",
        "--summary",
        "--log-level",
        "debug",
        "--log-format",
        "json",
    ]);
    assert!(warning_summary.status.success());
    let warning_lines: Vec<serde_json::Value> = String::from_utf8(warning_summary.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(warning_lines.iter().any(|line| line["kind"] == "warning"));
    assert!(warning_lines.iter().any(|line| line["kind"] == "summary"));
    assert!(warning_lines.iter().all(|line| line.get("kind").is_some()));
    let _ = std::fs::remove_file(warning_path);
}

#[test]
fn logging_configuration_is_bounded_and_cli_precedes_environment() {
    let invalid = run_with_log_env(&["--list", "a"], "not-a-level");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("LOG_LEVEL_INVALID"));

    let explicit = run_with_log_env(&["--list", "a", "--log-level", "off"], "not-a-level");
    assert!(explicit.status.success());
    assert_eq!(explicit.stdout, b"a\n");
    assert!(explicit.stderr.is_empty());
}

#[test]
fn machine_output_requires_json_log_framing_when_logging_is_enabled() {
    let out = run(&["--list", "a", "--format", "jsonl", "--log-level", "info"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("LOG_FORMAT_REQUIRED"));
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

#[test]
fn about_output_contains_release_and_bug_report_information() {
    let out = run(&["--about"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("tz_combinator"));
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("MIT"));
    assert!(text.contains("https://github.com/taggedzi/tz_combinator"));
    assert!(text.contains("https://github.com/taggedzi/tz_combinator/issues"));
    assert!(text.contains("Runtime:"));
    assert!(out.stderr.is_empty());
}

#[test]
fn help_contains_about_information_and_flag() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Version:"));
    assert!(text.contains("License: MIT"));
    assert!(text.contains("https://github.com/taggedzi/tz_combinator"));
    assert!(text.contains("--about"));
    assert!(text.contains("--allow-unsafe-terminal-output"));
    assert!(text.contains("--log-level"));
    assert!(text.contains("--log-format"));
    assert!(out.stderr.is_empty());
}
