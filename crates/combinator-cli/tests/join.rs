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
