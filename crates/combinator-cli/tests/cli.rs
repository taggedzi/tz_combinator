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
fn zero_timeout_cancels_before_output() {
    let output = bin()
        .args(["--list", "a,b", "--timeout-ms", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("CANCELLED"));
    assert!(output.stdout.is_empty());
}

#[test]
fn offset_at_end_creates_empty_output_file() {
    let path =
        std::env::temp_dir().join(format!("combinator_offset_end_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = bin()
        .args([
            "--list",
            "a,b",
            "--offset",
            "2",
            "--output",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(std::fs::read(&path).unwrap().is_empty());
    let _ = std::fs::remove_file(path);
}

#[test]
fn shards_cover_product_without_gaps_or_duplicates() {
    let all = bin()
        .args(["--list", "a,b", "--list", "1,2,3", "--sep", "-"])
        .output()
        .unwrap();
    let mut sharded = String::new();
    for index in 0..3 {
        let out = bin()
            .args([
                "--list",
                "a,b",
                "--list",
                "1,2,3",
                "--sep",
                "-",
                "--shard-index",
                &index.to_string(),
                "--shard-count",
                "3",
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        sharded.push_str(&String::from_utf8_lossy(&out.stdout));
    }
    assert_eq!(sharded, String::from_utf8_lossy(&all.stdout));
}

#[test]
fn shards_follow_reverse_output_order() {
    let out = bin()
        .args([
            "--list",
            "a,b",
            "--list",
            "1,2,3",
            "--sep",
            "-",
            "--reverse",
            "--shard-index",
            "0",
            "--shard-count",
            "2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b-3\nb-2\nb-1\n");
}

#[test]
fn explain_reports_effective_shard_page() {
    let out = bin()
        .args([
            "--list",
            "a,b",
            "--list",
            "1,2,3",
            "--format",
            "json",
            "--explain",
            "--shard-index",
            "1",
            "--shard-count",
            "2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["offset"], 3);
    assert_eq!(json["limit"], 3);
    assert_eq!(json["records_to_emit"], 3);
    assert_eq!(json["shard"]["start"], 3);
    assert_eq!(json["shard"]["end"], 6);
}

#[test]
fn invalid_shard_arguments_fail_before_input_use() {
    let out = bin()
        .args([
            "--file",
            "missing-input",
            "--shard-count",
            "0",
            "--shard-index",
            "0",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("SHARD_COUNT_INVALID"));
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
fn no_arguments_print_help_successfully() {
    let out = bin().output().unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("Usage:"), "stdout was: {help}");
    assert!(help.contains("--help"), "stdout was: {help}");
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
fn runtime_output_failure_does_not_commit_or_replace_output_file() {
    let path = std::env::temp_dir().join(format!(
        "combinator_runtime_atomic_{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, b"old").unwrap();
    let out = bin()
        .args([
            "--list",
            "a,b",
            "--output",
            path.to_str().unwrap(),
            "--overwrite",
            "--max-output-bytes",
            "2",
            "--no-preflight",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("OUTPUT_LIMIT_EXCEEDED"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let _ = std::fs::remove_file(path);
}

#[test]
fn template_file_limit_and_utf8_validation_are_enforced() {
    let oversized = std::env::temp_dir().join(format!(
        "combinator_template_large_{}.txt",
        std::process::id()
    ));
    std::fs::write(&oversized, b"{0}").unwrap();
    let out = bin()
        .args([
            "--list",
            "a",
            "--template-file",
            oversized.to_str().unwrap(),
            "--max-input-bytes",
            "2",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEMPLATE_TOO_LARGE"));

    let invalid = std::env::temp_dir().join(format!(
        "combinator_template_utf8_{}.txt",
        std::process::id()
    ));
    std::fs::write(&invalid, [0xff, 0xfe]).unwrap();
    let out = bin()
        .args(["--list", "a", "--template-file", invalid.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TEMPLATE_FILE_UNREADABLE"));
    let _ = std::fs::remove_file(oversized);
    let _ = std::fs::remove_file(invalid);
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

#[test]
fn explain_json_reports_bounded_plan_without_generating_records() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_combinator"))
        .args([
            "--list",
            "a,b",
            "--list",
            "c,d,e",
            "--offset",
            "1",
            "--limit",
            "2",
            "--sep",
            "-",
            "--explain",
            "--format",
            "json",
        ])
        .output()
        .expect("run combinator");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["schema_version"], 1);
    assert_eq!(summary["operation"], "product");
    assert_eq!(summary["combination_count"], 6);
    assert_eq!(summary["records_to_emit"], 2);
    assert_eq!(summary["output"], "stdout");
    assert_eq!(summary["format"], "json");
}

#[test]
fn dry_run_does_not_create_output_file() {
    let path = std::env::temp_dir().join(format!("combinator_dry_run_{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_combinator"))
        .args(["--list", "a,b", "--dry-run", "-o", path.to_str().unwrap()])
        .output()
        .expect("run combinator");
    assert!(output.status.success());
    assert!(!path.exists());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("combination_count=2"));
}

#[test]
fn json_format_requires_explain_or_dry_run() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_combinator"))
        .args(["--list", "a", "--format", "json"])
        .output()
        .expect("run combinator");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("FORMAT_UNSUPPORTED"));
}

#[test]
fn quiet_suppresses_non_fatal_warnings() {
    let path = std::env::temp_dir().join(format!("combinator_quiet_{}.txt", std::process::id()));
    std::fs::write(&path, "").unwrap();
    let output = bin()
        .args(["--file", path.to_str().unwrap(), "--quiet"])
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn warnings_as_errors_prevents_output_and_preserves_context() {
    let path = std::env::temp_dir().join(format!(
        "combinator_warning_error_{}.txt",
        std::process::id()
    ));
    let output_path = std::env::temp_dir().join(format!(
        "combinator_warning_output_{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, "").unwrap();
    let output = bin()
        .args([
            "--file",
            path.to_str().unwrap(),
            "--warnings-as-errors",
            "--output",
            output_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&output_path).ok();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("EMPTY_LIST"));
    assert!(stderr.contains("list_index=0"));
    assert!(!output_path.exists());
}

#[test]
fn escaped_inline_input_preserves_delimiters_and_decodes_escapes() {
    let out = bin()
        .args([
            "--input-format",
            "inline",
            "--list",
            r"a\,b,c\n",
            "--format",
            "nul",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a,b\0c\n\0");
}

#[test]
fn csv_input_accepts_quoted_delimiters_and_csv_output_quotes_fields() {
    let input = std::env::temp_dir().join(format!("combinator_f2_csv_{}.csv", std::process::id()));
    let second = std::env::temp_dir().join(format!(
        "combinator_f2_csv_second_{}.csv",
        std::process::id()
    ));
    std::fs::write(&input, "\"a,b\"\nplain\n").unwrap();
    std::fs::write(&second, "x\n").unwrap();
    let out = bin()
        .args([
            "--input-format",
            "csv",
            "--file",
            input.to_str().unwrap(),
            "--file",
            second.to_str().unwrap(),
            "--format",
            "csv",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"\"a,b\",x\nplain,x\n");
    let _ = std::fs::remove_file(input);
    let _ = std::fs::remove_file(second);
}

#[test]
fn nul_input_keeps_newlines_inside_records() {
    let input = std::env::temp_dir().join(format!("combinator_f2_nul_{}.dat", std::process::id()));
    std::fs::write(&input, b"first\nline\0second\0").unwrap();
    let out = bin()
        .args([
            "--input-format",
            "nul",
            "--file",
            input.to_str().unwrap(),
            "--format",
            "nul",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"first\nline\0second\0");
    let _ = std::fs::remove_file(input);
}

#[test]
fn mixed_sources_require_opt_in_and_reject_duplicate_stdin() {
    let input =
        std::env::temp_dir().join(format!("combinator_f2_mixed_{}.txt", std::process::id()));
    std::fs::write(&input, "file\n").unwrap();
    let mixed = bin()
        .args(["--list", "inline", "--file", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(mixed.status.code(), Some(2));
    let duplicate = bin().args(["--file", "-", "--file", "-"]).output().unwrap();
    assert_eq!(duplicate.status.code(), Some(2));
    let _ = std::fs::remove_file(input);
}

#[test]
fn malformed_csv_is_rejected_before_output_file_creation() {
    let input =
        std::env::temp_dir().join(format!("combinator_f2_bad_csv_{}.csv", std::process::id()));
    let output =
        std::env::temp_dir().join(format!("combinator_f2_bad_csv_{}.out", std::process::id()));
    let _ = std::fs::remove_file(&output);
    std::fs::write(&input, b"first,second\n").unwrap();
    let out = bin()
        .args([
            "--input-format",
            "csv",
            "--file",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(!output.exists());
    let _ = std::fs::remove_file(input);
}

#[test]
fn summary_is_stderr_only() {
    let output = bin().args(["--list", "a,b", "--summary"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "a\nb\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "summary[OUTPUT]: records=2, bytes=4\n"
    );
}

#[test]
fn completion_and_man_subcommands_generate_stdout() {
    let completion = bin().args(["completions", "bash"]).output().unwrap();
    assert!(completion.status.success());
    assert!(String::from_utf8_lossy(&completion.stdout).contains("combinator"));
    assert!(completion.stderr.is_empty());

    let man = bin().args(["man"]).output().unwrap();
    assert!(man.status.success());
    assert!(String::from_utf8_lossy(&man.stdout).contains(".TH combinator"));
    assert!(man.stderr.is_empty());
}

#[test]
fn closed_stdout_is_a_clean_cancellation() {
    use std::process::Stdio;

    let mut child = bin()
        .args([
            "--list",
            &(0..10_000).map(|_| "x").collect::<Vec<_>>().join(","),
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "closed stdout should not be a write error"
    );
}

#[test]
fn transforms_are_applied_per_list_in_argument_order() {
    let out = bin()
        .args([
            "--list",
            " B ,a,b,a,other ",
            "--transform",
            "trim",
            "--transform",
            "lower",
            "--transform",
            "filter=?",
            "--transform",
            "deduplicate",
            "--transform",
            "sort",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\n");
}

#[test]
fn malformed_transform_is_rejected_before_output() {
    let out = bin()
        .args(["--list", "a,b", "--transform", "filter=[a]"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("TRANSFORM_INVALID"));
    assert!(out.stdout.is_empty());
}

#[test]
fn explain_reports_transforms_and_normalized_sizes() {
    let out = bin()
        .args([
            "--list",
            " b ,a,a ",
            "--transform",
            "trim",
            "--transform",
            "deduplicate",
            "--explain",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        summary["transforms"],
        serde_json::json!(["trim", "deduplicate"])
    );
    assert_eq!(summary["input"]["items_per_list"], serde_json::json!([2]));
}
