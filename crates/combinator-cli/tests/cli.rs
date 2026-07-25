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
fn reverse_reverses_complete_product() {
    let out = bin()
        .args(["--list", "red,blue", "--list", "car,bike", "--sep", "-", "--reverse"])
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
        .args(["--list", "red,blue", "--list", "car,bike", "--sep", "-", "--reverse-fields"])
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
        .args(["--list", "a,b", "--list", "c,d", "--reverse", "--reverse-fields"])
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
    let out = bin().args(["--file", path.to_str().unwrap()]).output().unwrap();
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
        .args(["--list", "a,b", "--list", "c,d", "--format", "jsonl", "--offset", "1", "--limit", "2"])
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
    let out = bin().args(["--list", "a,b", "--sep", &big]).output().unwrap();
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
        .args(["--list", l1, "--list", l2, "-o", path_full.to_str().unwrap(), "--max-file-size", "100"])
        .output().unwrap();
    // --limit 20 -> 60 bytes, within the limit.
    let out_ltd = bin()
        .args(["--list", l1, "--list", l2, "--limit", "20", "-o", path_ltd.to_str().unwrap(), "--max-file-size", "100"])
        .output().unwrap();

    let full_code = out_full.status.code();
    let full_err = String::from_utf8_lossy(&out_full.stderr).into_owned();
    let ltd_ok = out_ltd.status.success();
    let ltd_lines = std::fs::read_to_string(&path_ltd).unwrap_or_default().lines().count();
    std::fs::remove_file(&path_full).ok();
    std::fs::remove_file(&path_ltd).ok();

    assert_eq!(full_code, Some(1), "unbounded write should hit the file-size limit");
    assert!(full_err.contains("FILE_SIZE_LIMIT"), "stderr: {full_err}");
    assert!(ltd_ok, "limited write should pass pre-flight");
    assert_eq!(ltd_lines, 20);
}
