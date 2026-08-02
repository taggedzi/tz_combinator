use std::ffi::OsString;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use combinator_benchmarks::values;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use tempfile::tempdir;

fn config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(250))
        .measurement_time(Duration::from_secs(1))
}

fn release_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("COMBINATOR_BENCH_BIN") {
        return PathBuf::from(path);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("benchmark package must be under workspace/crates");
    let executable = if cfg!(windows) {
        "combinator.exe"
    } else {
        "combinator"
    };
    workspace.join("target").join("release").join(executable)
}

fn invoke(binary: &Path, arguments: &[OsString]) -> Output {
    Command::new(binary)
        .args(arguments)
        .output()
        .expect("run release combinator binary")
}

fn assert_success(output: &Output, expected_stdout_records: Option<usize>) {
    assert!(
        output.status.success(),
        "CLI benchmark failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if let Some(expected) = expected_stdout_records {
        let actual = output.stdout.iter().filter(|byte| **byte == b'\n').count();
        assert_eq!(actual, expected, "CLI benchmark output count changed");
    }
}

fn time_validated_invocations(
    iterations: u64,
    binary: &Path,
    arguments: &[OsString],
    expected_stdout_records: Option<usize>,
) -> Duration {
    let mut elapsed = Duration::ZERO;
    for _ in 0..iterations {
        let started = Instant::now();
        let output = invoke(binary, arguments);
        let duration = started.elapsed();
        assert_success(&output, expected_stdout_records);
        black_box((&output.stdout, &output.stderr));
        elapsed = elapsed
            .checked_add(duration)
            .expect("bounded benchmark duration must fit");
    }
    elapsed
}

fn strings(arguments: &[&str]) -> Vec<OsString> {
    arguments.iter().map(OsString::from).collect()
}

fn cli_benchmarks(criterion: &mut Criterion) {
    let binary = release_binary();
    assert!(
        binary.is_file(),
        "release CLI missing at {}; run `cargo build -p combinator-cli --release --locked` first or set COMBINATOR_BENCH_BIN",
        binary.display()
    );

    let startup = strings(&["--version"]);
    assert_success(&invoke(&binary, &startup), Some(1));

    let small = strings(&[
        "product", "--list", "red,blue", "--list", "car,bike", "--sep", "-",
    ]);
    assert_success(&invoke(&binary, &small), Some(4));

    let left = values("left", 32, 16).join(",");
    let right = values("right", 32, 16).join(",");
    let medium = vec![
        "product".into(),
        "--list".into(),
        left.into(),
        "--list".into(),
        right.into(),
        "--sep".into(),
        "|".into(),
        "--limit".into(),
        "1024".into(),
    ];
    assert_success(&invoke(&binary, &medium), Some(1_024));

    let input_directory = tempdir().expect("create dedicated CLI input directory");
    let input_path = input_directory.path().join("escaping.csv");
    let csv = (0..512)
        .map(|index| format!("\"item-{index:08},\"\"quoted\"\"\"\n"))
        .collect::<String>();
    std::fs::write(&input_path, csv).expect("write bounded CLI fixture");
    let codec = vec![
        "product".into(),
        "--file".into(),
        input_path.as_os_str().to_owned(),
        "--input-format".into(),
        "csv".into(),
        "--format".into(),
        "jsonl".into(),
        "--limit".into(),
        "512".into(),
    ];
    assert_success(&invoke(&binary, &codec), Some(512));

    let mut group = criterion.benchmark_group("cli/release");
    group.bench_function("startup/version", |bencher| {
        bencher.iter_custom(|iterations| {
            time_validated_invocations(iterations, &binary, &startup, Some(1))
        });
    });
    group.throughput(Throughput::Elements(4));
    group.bench_function("small/product", |bencher| {
        bencher.iter_custom(|iterations| {
            time_validated_invocations(iterations, &binary, &small, Some(4))
        });
    });
    group.throughput(Throughput::Elements(1_024));
    group.bench_function("medium/product", |bencher| {
        bencher.iter_custom(|iterations| {
            time_validated_invocations(iterations, &binary, &medium, Some(1_024))
        });
    });
    group.throughput(Throughput::Elements(512));
    group.bench_function("medium/codec-heavy-csv-to-jsonl", |bencher| {
        bencher.iter_custom(|iterations| {
            time_validated_invocations(iterations, &binary, &codec, Some(512))
        });
    });
    group.finish();

    let file_left = values("left", 32, 16).join(",");
    let file_right = values("right", 32, 16).join(",");
    let mut file_group = criterion.benchmark_group("cli/release/file-output");
    file_group.throughput(Throughput::Elements(1_024));
    file_group.bench_function("create-new", |bencher| {
        bencher.iter_custom(|iterations| {
            let mut elapsed = Duration::ZERO;
            for _ in 0..iterations {
                let directory = tempdir().expect("create dedicated CLI output directory");
                let output_path = directory.path().join("output.txt");
                let arguments = vec![
                    "product".into(),
                    "--list".into(),
                    file_left.clone().into(),
                    "--list".into(),
                    file_right.clone().into(),
                    "--output".into(),
                    output_path.as_os_str().to_owned(),
                    "--limit".into(),
                    "1024".into(),
                ];
                let started = Instant::now();
                let output = invoke(&binary, &arguments);
                let duration = started.elapsed();
                assert_success(&output, Some(0));
                let written = std::fs::read(&output_path).expect("read CLI benchmark output");
                assert_eq!(written.iter().filter(|byte| **byte == b'\n').count(), 1_024);
                black_box(written);
                elapsed = elapsed
                    .checked_add(duration)
                    .expect("bounded benchmark duration must fit");
            }
            elapsed
        });
    });
    file_group.finish();
}

criterion_group! {
    name = benches;
    config = config();
    targets = cli_benchmarks
}
criterion_main!(benches);
