use std::fs;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

fn paths(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir();
    let stem = format!("combinator_f8_{name}_{}", std::process::id());
    (
        dir.join(format!("{stem}_left.csv")),
        dir.join(format!("{stem}_right.csv")),
    )
}

#[test]
fn join_limits_cannot_raise_shared_hard_ceilings() {
    for (flag, value) in [
        (
            "--max-join-records",
            (combinator_app::HARD_MAX_JOIN_RECORDS + 1).to_string(),
        ),
        (
            "--max-join-key-fanout",
            (combinator_app::HARD_MAX_JOIN_KEY_FANOUT + 1).to_string(),
        ),
    ] {
        let output = bin()
            .args([
                "join",
                "--left",
                "missing-left.csv",
                "--right",
                "missing-right.csv",
                "--left-key",
                "id",
                "--right-key",
                "id",
                "--format",
                "jsonl",
                flag,
                &value,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{flag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("RESOURCE_LIMIT_TOO_HIGH"),
            "{flag}: {stderr}"
        );
        assert!(
            stderr.contains(flag.trim_start_matches("--")),
            "{flag}: {stderr}"
        );
        assert!(!stderr.contains("FILE_UNREADABLE"), "{flag}: {stderr}");
    }
}

#[test]
fn left_join_expands_duplicates_and_renames_collisions() {
    let (left, right) = paths("duplicate");
    fs::write(&left, "id,name\n1,A\n2,B\n").unwrap();
    fs::write(&right, "id,name\n1,X\n1,Y\n").unwrap();
    let output = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--type",
            "left",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\"name_right\":\"X\""));
    assert!(text.contains("\"name_right\":\"Y\""));
    assert!(text.contains("\"id\":\"2\""));
    assert!(text.contains("\"name\":\"B\""));
    assert!(text.contains("\"id_right\":null"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn malformed_jsonl_is_rejected_before_output() {
    let (left, right) = paths("malformed");
    fs::write(&left, "{bad\n").unwrap();
    fs::write(&right, "{\"id\":\"1\"}\n").unwrap();
    let output = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "jsonl",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("JSONL_MALFORMED"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn join_stdin_respects_input_limit() {
    use std::io::Write;

    let (left, right) = paths("stdin_limit");
    fs::write(&left, "id\n1\n").unwrap();
    fs::write(&right, "id\n1\n").unwrap();
    let mut child = bin()
        .args([
            "join",
            "--left",
            "-",
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "csv",
            "--format",
            "jsonl",
            "--max-input-bytes",
            "4",
        ])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"id\n1\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("INPUT_TOO_LARGE"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn full_and_anti_join_cover_unmatched_records() {
    let (left, right) = paths("kinds");
    fs::write(&left, "id,value\n1,left\n2,only-left\n").unwrap();
    fs::write(&right, "id,value\n1,right\n3,only-right\n").unwrap();

    let full = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--type",
            "full",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        full.status.success(),
        "{}",
        String::from_utf8_lossy(&full.stderr)
    );
    let full_text = String::from_utf8(full.stdout).unwrap();
    assert!(full_text.contains("only-left"));
    assert!(full_text.contains("only-right"));

    let anti = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--type",
            "anti",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(
        anti.status.success(),
        "{}",
        String::from_utf8_lossy(&anti.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&anti.stdout).lines().count(), 1);
    assert!(String::from_utf8_lossy(&anti.stdout).contains("only-left"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn join_rejects_non_string_json_fields_and_bad_csv_schema() {
    let (left, right) = paths("invalid_records");
    fs::write(&left, "{\"id\":1}\n").unwrap();
    fs::write(&right, "{\"id\":\"1\"}\n").unwrap();
    let json = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "jsonl",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(!json.status.success());
    assert!(String::from_utf8_lossy(&json.stderr).contains("JOIN_FIELD_INVALID"));

    fs::write(&left, "id,,value\n1,x,y\n").unwrap();
    fs::write(&right, "id,value\n1,z\n").unwrap();
    let csv = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "csv",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(!csv.status.success());
    assert!(String::from_utf8_lossy(&csv.stderr).contains("JOIN_SCHEMA_INVALID"));

    fs::write(&left, "id,value\n1\n").unwrap();
    fs::write(&right, "id,value\n1,z\n").unwrap();
    let short_row = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "csv",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(!short_row.status.success());
    assert!(
        String::from_utf8_lossy(&short_row.stderr).contains("CSV_MALFORMED"),
        "stderr: {}",
        String::from_utf8_lossy(&short_row.stderr)
    );

    fs::write(&left, "[]\n").unwrap();
    let scalar = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--join-format",
            "jsonl",
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    assert!(!scalar.status.success());
    assert!(String::from_utf8_lossy(&scalar.stderr).contains("JOIN_RECORD_INVALID"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn join_offset_and_zero_limit_emit_nothing() {
    let (left, right) = paths("pagination");
    fs::write(&left, "id\n1\n2\n").unwrap();
    fs::write(&right, "id\n1\n2\n").unwrap();
    for args in [vec!["--offset", "9"], vec!["--limit", "0"]] {
        let mut command = vec![
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--format",
            "jsonl",
        ];
        command.extend(args);
        let output = bin().args(command).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}

#[test]
fn join_limits_records_and_duplicate_key_expansion() {
    let (left, right) = paths("resource_limits");
    fs::write(&left, "id\n1\n1\n").unwrap();
    fs::write(&right, "id\n1\n1\n").unwrap();
    let output = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--format",
            "jsonl",
            "--max-join-key-fanout",
            "3",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("JOIN_FANOUT_LIMIT_EXCEEDED"));

    let output = bin()
        .args([
            "join",
            "--left",
            left.to_str().unwrap(),
            "--right",
            right.to_str().unwrap(),
            "--left-key",
            "id",
            "--right-key",
            "id",
            "--format",
            "jsonl",
            "--max-join-records",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("TOO_MANY_ITEMS"));
    let _ = fs::remove_file(left);
    let _ = fs::remove_file(right);
}
