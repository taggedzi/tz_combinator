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
