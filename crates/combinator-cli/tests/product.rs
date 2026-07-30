use std::process::{Command, Output};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

fn run(args: &[&str]) -> Output {
    bin().args(args).output().expect("run combinator")
}

#[test]
fn product_combines_separator_record_separator_and_three_lists() {
    let output = run(&[
        "product",
        "--list",
        "a,b",
        "--list",
        "1,2",
        "--list",
        "x,y",
        "--sep",
        ":",
        "--rec-sep",
        "|",
    ]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "a:1:x|a:1:y|a:2:x|a:2:y|b:1:x|b:1:y|b:2:x|b:2:y|"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn product_combines_reverse_offset_and_limit() {
    let output = run(&[
        "product",
        "--list",
        "a,b",
        "--list",
        "1,2,3",
        "--sep",
        "",
        "--reverse",
        "--offset",
        "1",
        "--limit",
        "3",
    ]);

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "b2\nb1\na3\n");
}

#[test]
fn product_supports_each_delimited_output_format() {
    let csv = run(&[
        "product", "--list", "a,b", "--list", "1,2", "--sep", ",", "--format", "csv",
    ]);
    assert!(csv.status.success());
    assert_eq!(
        String::from_utf8(csv.stdout).unwrap(),
        "a,1\na,2\nb,1\nb,2\n"
    );

    let tsv = run(&[
        "product", "--list", "a,b", "--list", "1,2", "--sep", "\t", "--format", "tsv",
    ]);
    assert!(tsv.status.success());
    assert_eq!(
        String::from_utf8(tsv.stdout).unwrap(),
        "a\t1\na\t2\nb\t1\nb\t2\n"
    );

    let nul = run(&[
        "product", "--list", "a,b", "--list", "1,2", "--sep", ":", "--format", "nul",
    ]);
    assert!(nul.status.success());
    assert_eq!(nul.stdout, b"a:1\0a:2\0b:1\0b:2\0");
}

#[test]
fn product_jsonl_window_keeps_field_and_name_metadata_aligned() {
    let output = run(&[
        "product", "--list", "a,b", "--list", "1,2", "--name", "host", "--name", "port",
        "--format", "jsonl", "--offset", "1", "--limit", "1",
    ]);

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "{\"i\":1,\"value\":\"a2\",\"fields\":[\"a\",\"2\"],\"named\":{\"host\":\"a\",\"port\":\"2\"}}\n"
    );
}

#[test]
fn product_combines_normalization_filter_and_template_in_order() {
    let output = run(&[
        "product",
        "--list",
        " b ,a ",
        "--list",
        "x,x,y",
        "--transform",
        "trim",
        "--transform",
        "deduplicate",
        "--filter",
        "prefix:0=a",
        "--template",
        "{0}/{1}",
    ]);

    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "a/x\na/y\n");
}

#[test]
fn product_inline_delimiter_and_file_input_are_independent_sources() {
    let directory = std::env::temp_dir().join(format!("combinator_product_{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("items.txt");
    std::fs::write(&path, "x\ny\n").unwrap();

    let path_string = path.to_string_lossy().into_owned();
    let output = bin()
        .args([
            "product",
            "--list",
            "left;right",
            "--list-delim",
            ";",
            "--file",
            &path_string,
            "--allow-mixed-inputs",
            "--sep",
            "-",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "left-x\nleft-y\nright-x\nright-y\n"
    );
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(&directory);
}

#[test]
fn product_rejects_conflicting_reverse_options_without_output() {
    let output = run(&[
        "product",
        "--list",
        "a,b",
        "--list",
        "1,2",
        "--reverse",
        "--reverse-fields",
        "--dry-run",
    ]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("REVERSE_CONFLICT"));
    assert!(output.stdout.is_empty());
}
