use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

/// Every listed argument set must exit with a code in {0,1,2} and never crash
/// (a panic aborts with a signal / code 101 and prints a backtrace).
#[test]
fn malformed_inputs_never_panic() {
    let long_sep = "x".repeat(9000);
    let cases: Vec<Vec<&str>> = vec![
        vec!["--file", "/nonexistent/path/nope.txt"],
        vec!["--list", "a,b", "--offset", "999999999999"],
        vec!["--list", "a,b", "--limit", "0"],
        vec!["--list", ""], // single empty item, not empty list
        vec!["--list", "a,b", "--list-delim", ""],
        vec!["--list", "a", "--sep", &long_sep],
        vec![
            "--list",
            "a,b",
            "--offset",
            "340282366920938463463374607431768211455",
        ],
    ];
    for args in cases {
        let out = bin().args(&args).output().unwrap();
        let code = out.status.code();
        assert!(
            matches!(code, Some(0) | Some(1) | Some(2)),
            "args {args:?} produced code {code:?} (stderr: {})",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("panicked"),
            "args {args:?} panicked"
        );
    }
}
