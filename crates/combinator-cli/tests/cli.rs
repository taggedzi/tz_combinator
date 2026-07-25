//! Black-box tests invoking the compiled `combinator` binary.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn basic_product_to_stdout() {
    let out = bin()
        .args(["--list", "red,blue", "--list", "car,bike", "--sep", "-"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "red-car\nred-bike\nblue-car\nblue-bike\n"
    );
}

#[test]
fn bare_product_matches_explicit_product_subcommand() {
    let args = ["--list", "a,b", "--list", "x,y", "--sep", "-"];
    let bare = bin().args(args).output().unwrap();
    let explicit = bin()
        .args(["product", "--list", "a,b", "--list", "x,y", "--sep", "-"])
        .output()
        .unwrap();

    assert_eq!(bare.status, explicit.status);
    assert_eq!(bare.stdout, explicit.stdout);
    assert_eq!(bare.stderr, explicit.stderr);
}

#[test]
fn product_template_renders_positional_fields() {
    let out = bin()
        .args([
            "product",
            "--list",
            "server1,server2",
            "--list",
            "80,443",
            "--template",
            "https://{0}:{1}",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "https://server1:80\nhttps://server1:443\nhttps://server2:80\nhttps://server2:443\n"
    );
}

#[test]
fn named_template_adds_json_metadata() {
    let out = bin()
        .args([
            "product",
            "--name",
            "host",
            "--name",
            "port",
            "--list",
            "server1",
            "--list",
            "443",
            "--template",
            "{host}:{port}",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "{\"i\":0,\"value\":\"server1:443\",\"fields\":[\"server1\",\"443\"],\"named\":{\"host\":\"server1\",\"port\":\"443\"}}\n"
    );
}

#[test]
fn template_file_renders_and_lean_jsonl_stays_lean() {
    let path = std::env::temp_dir().join(format!("combinator_template_{}.txt", std::process::id()));
    std::fs::write(&path, "{0}@{1}").unwrap();
    let out = bin()
        .args([
            "--list",
            "host",
            "--list",
            "port",
            "--template-file",
            path.to_str().unwrap(),
            "--format",
            "jsonl",
            "--lean-output",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "\"host@port\"\n");
}

#[test]
fn template_validation_errors_are_stable_and_prevent_output_creation() {
    let output = std::env::temp_dir().join(format!(
        "combinator_template_invalid_{}.txt",
        std::process::id()
    ));
    let out = bin()
        .args([
            "--list",
            "a",
            "--template",
            "{missing}",
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEMPLATE_UNKNOWN_FIELD"));
    assert!(!output.exists());
}

#[test]
fn template_and_separator_conflict_is_a_usage_error() {
    let out = bin()
        .args(["--list", "a", "--template", "{0}", "--sep", "-"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEMPLATE_SEPARATOR_CONFLICT"));
}

#[test]
fn template_source_conflict_is_a_usage_error() {
    let path = std::env::temp_dir().join(format!(
        "combinator_template_conflict_{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, "{0}").unwrap();
    let out = bin()
        .args([
            "--list",
            "a",
            "--template",
            "{0}",
            "--template-file",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEMPLATE_CONFLICT"));
}

#[test]
fn template_names_must_be_unique_and_match_list_count() {
    let duplicate = bin()
        .args([
            "--name",
            "host",
            "--name",
            "host",
            "--list",
            "a",
            "--list",
            "b",
            "--template",
            "{host}",
        ])
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("TEMPLATE_DUPLICATE_NAME"));

    let mismatch = bin()
        .args([
            "--name",
            "host",
            "--list",
            "a",
            "--list",
            "b",
            "--template",
            "{host}",
        ])
        .output()
        .unwrap();
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("TEMPLATE_NAMES_MISMATCH"));
}

#[test]
fn template_expansion_is_subject_to_output_limit() {
    let out = bin()
        .args([
            "--list",
            "x",
            "--template",
            "long-{0}",
            "--max-output-bytes",
            "4",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_LIMIT_EXCEEDED"));
    assert!(out.stdout.is_empty());
}

#[test]
fn template_preflight_accounts_for_literal_expansion() {
    let output = std::env::temp_dir().join(format!(
        "combinator_template_preflight_{}.txt",
        std::process::id()
    ));
    let out = bin()
        .args([
            "--list",
            "x",
            "--template",
            "long-{0}",
            "--output",
            output.to_str().unwrap(),
            "--max-file-size",
            "4",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&output).ok();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("FILE_SIZE_LIMIT"));
}

#[test]
fn reverse_reverses_complete_product() {
    let out = bin()
        .args([
            "--list",
            "red,blue",
            "--list",
            "car,bike",
            "--sep",
            "-",
            "--reverse",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "blue-bike\nblue-car\nred-bike\nred-car\n"
    );
}

#[test]
fn reverse_fields_preserves_previous_order() {
    let out = bin()
        .args([
            "--list",
            "red,blue",
            "--list",
            "car,bike",
            "--sep",
            "-",
            "--reverse-fields",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "red-car\nblue-car\nred-bike\nblue-bike\n"
    );
}

#[test]
fn reverse_modes_conflict() {
    let out = bin()
        .args([
            "--list",
            "a,b",
            "--list",
            "c,d",
            "--reverse",
            "--reverse-fields",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("REVERSE_CONFLICT"));
}

#[test]
fn count_only_prints_total() {
    let out = bin()
        .args(["--list", "a,b", "--list", "c,d,e", "--count-only"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "6");
}

#[test]
fn no_lists_is_usage_error() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("NO_LISTS"), "stderr was: {err}");
}

#[test]
fn mixing_list_and_file_is_source_conflict() {
    let out = bin()
        .args(["--list", "a,b", "--file", "some.txt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("SOURCE_CONFLICT"));
}

#[test]
fn reads_list_from_stdin_dash() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = bin()
        .args(["--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"a\nb\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\n");
}

#[test]
fn empty_list_warns_and_exits_zero() {
    // An inline empty value produces a single empty item, not an empty list, so
    // use a file with no lines to get a truly empty list.
    let path = std::env::temp_dir().join("combinator_e2e_empty.txt");
    std::fs::write(&path, "").unwrap();
    let out = bin()
        .args(["--file", path.to_str().unwrap()])
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("EMPTY_LIST"));
}

#[test]
fn output_file_exists_without_overwrite_errors() {
    let path = std::env::temp_dir().join("combinator_e2e_exists.txt");
    std::fs::write(&path, "old").unwrap();
    let out = bin()
        .args(["--list", "a,b", "-o", path.to_str().unwrap()])
        .output()
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_EXISTS"));
    assert_eq!(contents, "old", "existing file must be untouched");
}

#[test]
fn overwrite_writes_file() {
    let path = std::env::temp_dir().join("combinator_e2e_overwrite.txt");
    std::fs::write(&path, "old").unwrap();
    let out = bin()
        .args(["--list", "a,b", "-o", path.to_str().unwrap(), "--overwrite"])
        .output()
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert_eq!(contents, "a\nb\n");
}

#[test]
fn jsonl_and_offset_limit() {
    let out = bin()
        .args([
            "--list", "a,b", "--list", "c,d", "--format", "jsonl", "--offset", "1", "--limit", "2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["value"], "ad");
    assert_eq!(first["i"], 1);
}

#[test]
fn oversized_delimiter_is_usage_error() {
    let big = "x".repeat(5000);
    let out = bin()
        .args(["--list", "a,b", "--sep", &big])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("BAD_DELIMITER"));
}

#[test]
fn preflight_size_check_respects_limit() {
    let dir = std::env::temp_dir();
    let path_full = dir.join("combinator_e2e_preflight_full.txt");
    let path_ltd = dir.join("combinator_e2e_preflight_ltd.txt");
    let l1 = "a,b,c,d,e,f,g,h,i,j"; // 10
    let l2 = "0,1,2,3,4,5,6,7,8,9"; // 10 -> 100 combos, each record "xy\n" = 3 bytes => 300 bytes full

    // Full product (300 bytes) exceeds the 100-byte file-size limit.
    let out_full = bin()
        .args([
            "--list",
            l1,
            "--list",
            l2,
            "-o",
            path_full.to_str().unwrap(),
            "--max-file-size",
            "100",
        ])
        .output()
        .unwrap();
    // --limit 20 -> 60 bytes, within the limit.
    let out_ltd = bin()
        .args([
            "--list",
            l1,
            "--list",
            l2,
            "--limit",
            "20",
            "-o",
            path_ltd.to_str().unwrap(),
            "--max-file-size",
            "100",
        ])
        .output()
        .unwrap();

    let full_code = out_full.status.code();
    let full_err = String::from_utf8_lossy(&out_full.stderr).into_owned();
    let ltd_ok = out_ltd.status.success();
    let ltd_lines = std::fs::read_to_string(&path_ltd)
        .unwrap_or_default()
        .lines()
        .count();
    std::fs::remove_file(&path_full).ok();
    std::fs::remove_file(&path_ltd).ok();

    assert_eq!(
        full_code,
        Some(1),
        "unbounded write should hit the file-size limit"
    );
    assert!(full_err.contains("FILE_SIZE_LIMIT"), "stderr: {full_err}");
    assert!(ltd_ok, "limited write should pass pre-flight");
    assert_eq!(ltd_lines, 20);
}

#[test]
fn runtime_output_limit_stops_streaming() {
    let out = bin()
        .args(["--list", "a,b", "--max-output-bytes", "3"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_LIMIT_EXCEEDED"));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\n");
}

#[test]
fn resource_limits_cannot_be_raised_above_hard_ceiling() {
    let out = bin()
        .args(["--list", "a", "--max-output-bytes", "18446744073709551615"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("RESOURCE_LIMIT_TOO_HIGH"));
}
