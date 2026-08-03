use std::hint::black_box;
use std::io::Cursor;
use std::time::Duration;

use combinator_app::{stream, AppOperation, FileSink, ProductRequest};
use combinator_benchmarks::{values, CountingSink, FixtureSize};
use combinator_codecs::{format_record, Format, InputBudget, InputFormat, InputLimits};
use combinator_core::ProductOptions;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tempfile::tempdir;

fn config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(250))
        .measurement_time(Duration::from_millis(750))
}

fn logical_values(count: usize, width: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            let prefix = format!("item-{index:08},\"quoted\"");
            format!("{prefix}{}", "x".repeat(width.saturating_sub(prefix.len())))
        })
        .collect()
}

fn encode_input(values: &[String], format: InputFormat) -> Vec<u8> {
    let mut encoded = Vec::new();
    for value in values {
        match format {
            InputFormat::Lines => {
                encoded.extend_from_slice(value.as_bytes());
                encoded.push(b'\n');
            }
            InputFormat::Nul => {
                encoded.extend_from_slice(value.as_bytes());
                encoded.push(0);
            }
            InputFormat::Csv | InputFormat::Tsv => {
                encoded.push(b'"');
                for byte in value.bytes() {
                    if byte == b'"' {
                        encoded.push(b'"');
                    }
                    encoded.push(byte);
                }
                encoded.extend_from_slice(b"\"\n");
            }
        }
    }
    encoded
}

fn codec_parse_benchmarks(criterion: &mut Criterion) {
    let shapes = [
        (FixtureSize::Small, FixtureSize::Small.records(), 24usize),
        (FixtureSize::Medium, FixtureSize::Medium.records(), 24usize),
        (FixtureSize::Large, 512usize, 256usize),
    ];
    let formats = [
        ("lines", InputFormat::Lines),
        ("csv", InputFormat::Csv),
        ("tsv", InputFormat::Tsv),
        ("nul-delimited", InputFormat::Nul),
    ];
    let mut group = criterion.benchmark_group("codecs/parse");
    for (size, count, width) in shapes {
        let values = logical_values(count, width);
        for (format_name, format) in formats {
            let encoded = encode_input(&values, format);
            let limits = InputLimits {
                max_input_bytes: encoded.len(),
                max_item_bytes: width.checked_mul(2).expect("bounded item size"),
                max_items_per_list: count,
            };
            let mut validation_budget = InputBudget::new(encoded.len(), count);
            let parsed = combinator_codecs::input::read_formatted(
                Cursor::new(encoded.as_slice()),
                "synthetic",
                format,
                limits,
                &mut validation_budget,
            )
            .expect("valid codec fixture");
            assert_eq!(
                parsed, values,
                "codec fixtures must be logically equivalent"
            );

            group.throughput(Throughput::Bytes(encoded.len() as u64));
            group.bench_with_input(
                BenchmarkId::new(format_name, format!("{}/{}x{}", size.label(), count, width)),
                &format,
                |bencher, format| {
                    bencher.iter(|| {
                        let mut budget = InputBudget::new(encoded.len(), count);
                        black_box(
                            combinator_codecs::input::read_formatted(
                                Cursor::new(black_box(encoded.as_slice())),
                                "synthetic",
                                *format,
                                limits,
                                &mut budget,
                            )
                            .expect("valid codec fixture"),
                        )
                    });
                },
            );
        }
    }
    group.finish();
}

fn codec_render_benchmarks(criterion: &mut Criterion) {
    struct Shape {
        name: &'static str,
        fields: Vec<String>,
    }
    let shapes = [
        Shape {
            name: "small/4-fields/narrow",
            fields: logical_values(4, 24),
        },
        Shape {
            name: "medium/16-fields/wide-escaping",
            fields: logical_values(16, 256),
        },
    ];
    let formats = [
        ("text", Format::Text),
        ("csv", Format::Csv),
        ("tsv", Format::Tsv),
        ("jsonl", Format::Jsonl),
        ("nul-delimited", Format::Nul),
    ];
    let mut group = criterion.benchmark_group("codecs/render");
    for shape in &shapes {
        let refs = shape.fields.iter().map(String::as_str).collect::<Vec<_>>();
        for (format_name, format) in formats {
            let validation = format_record(&refs, 42, "|", "\n", format, false, 1024 * 1024)
                .expect("valid render fixture");
            assert!(!validation.is_empty());
            group.throughput(Throughput::Bytes(validation.len() as u64));
            group.bench_with_input(
                BenchmarkId::new(format_name, shape.name),
                &format,
                |bencher, format| {
                    bencher.iter(|| {
                        black_box(
                            format_record(
                                black_box(&refs),
                                black_box(42),
                                "|",
                                "\n",
                                *format,
                                false,
                                1024 * 1024,
                            )
                            .expect("valid render fixture"),
                        )
                    });
                },
            );
        }
    }
    group.finish();
}

fn application_request(format: Format) -> ProductRequest {
    ProductRequest {
        lists: vec![values("left", 32, 24), values("right", 32, 24)],
        field_separator: "|".to_string(),
        record_separator: "\n".to_string(),
        format,
        options: ProductOptions {
            limit: Some(512),
            ..ProductOptions::default()
        },
        operation: AppOperation::Product {
            reverse_fields: false,
        },
        max_combinations: 512,
        max_output_bytes: 16 * 1024 * 1024,
        ..ProductRequest::default()
    }
}

fn application_stream_benchmarks(criterion: &mut Criterion) {
    let cases = [
        ("text", Format::Text),
        ("csv", Format::Csv),
        ("jsonl", Format::Jsonl),
    ];
    let mut group = criterion.benchmark_group("application/stream/counting-sink");
    for (name, format) in cases {
        let request = application_request(format);
        let mut validation_sink = CountingSink::default();
        let validation =
            stream(&request, &mut validation_sink, None).expect("valid application fixture");
        assert_eq!(validation.records, 512);
        assert_eq!(validation_sink.records, 512);
        group.throughput(Throughput::Elements(512));
        group.bench_function(name, |bencher| {
            bencher.iter(|| {
                let mut sink = CountingSink::default();
                let progress = stream(black_box(&request), &mut sink, None)
                    .expect("valid application fixture");
                black_box((progress, sink.records, sink.bytes, sink.checksum))
            });
        });
    }
    group.finish();
}

fn write_application_file(request: &ProductRequest, overwrite: bool) -> (u128, u64) {
    let directory = tempdir().expect("create dedicated benchmark directory");
    let output = directory.path().join("output.txt");
    if overwrite {
        std::fs::write(&output, b"existing").expect("seed replacement destination");
    }
    let mut sink = FileSink::open(&output, overwrite).expect("open safe benchmark output");
    let progress = stream(request, &mut sink, None).expect("stream application fixture");
    sink.commit().expect("commit benchmark output");
    let bytes = std::fs::metadata(&output)
        .expect("stat committed benchmark output")
        .len();
    assert_eq!(u128::from(bytes), progress.bytes);
    (progress.records, bytes)
}

fn application_file_benchmarks(criterion: &mut Criterion) {
    let request = application_request(Format::Text);
    for overwrite in [false, true] {
        let validation = write_application_file(&request, overwrite);
        assert_eq!(validation.0, 512);
    }
    let mut group = criterion.benchmark_group("application/stream/file-sink");
    group.sample_size(10);
    group.throughput(Throughput::Elements(512));
    group.bench_function("create-new", |bencher| {
        bencher.iter(|| black_box(write_application_file(black_box(&request), false)));
    });
    group.bench_function("safe-replacement", |bencher| {
        bencher.iter(|| black_box(write_application_file(black_box(&request), true)));
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = config();
    targets = codec_parse_benchmarks, codec_render_benchmarks, application_stream_benchmarks, application_file_benchmarks
}
criterion_main!(benches);
