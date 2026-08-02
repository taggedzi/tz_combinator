use std::hint::black_box;
use std::time::Duration;

use combinator_benchmarks::{checked_product, join_records, lists};
use combinator_core::{
    combinations as product_records, concat_records, join_count_with_fanout, join_each_with_fanout,
    permutations, select_combinations, variations, zip_records, ConcatOptions, CoreError, JoinType,
    ProductOptions, SelectionOptions, UnequalPolicy, ZipOptions,
};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(250))
        .measurement_time(Duration::from_millis(750))
}

fn consume_index_vectors<I>(records: I) -> (u128, usize)
where
    I: IntoIterator<Item = Vec<usize>>,
{
    records
        .into_iter()
        .fold((0u128, 0usize), |(count, checksum), record| {
            let count = count
                .checked_add(1)
                .expect("bounded fixture count must fit in u128");
            let checksum = record
                .into_iter()
                .fold(checksum, |value, index| value.wrapping_add(index));
            (count, checksum)
        })
}

fn product_benchmarks(criterion: &mut Criterion) {
    struct Case {
        name: &'static str,
        lengths: Vec<usize>,
        options: ProductOptions,
    }

    let cases = [
        Case {
            name: "small/2-fields/forward",
            lengths: vec![16, 16],
            options: ProductOptions {
                limit: Some(128),
                ..ProductOptions::default()
            },
        },
        Case {
            name: "medium/8-fields/forward",
            lengths: vec![4; 8],
            options: ProductOptions {
                limit: Some(256),
                ..ProductOptions::default()
            },
        },
        Case {
            name: "medium/32-fields/reverse",
            lengths: vec![2; 32],
            options: ProductOptions {
                reverse: true,
                limit: Some(128),
                ..ProductOptions::default()
            },
        },
        Case {
            name: "medium/ragged/reverse-fields",
            lengths: vec![2, 3, 5, 7],
            options: ProductOptions {
                reverse_fields: true,
                limit: Some(210),
                ..ProductOptions::default()
            },
        },
        Case {
            name: "large-logical/high-offset",
            lengths: vec![10; 12],
            options: ProductOptions {
                offset: 999_999_999_500,
                limit: Some(256),
                ..ProductOptions::default()
            },
        },
    ];

    let mut setup = criterion.benchmark_group("core/product/setup");
    for case in &cases {
        let fixture = lists(&case.lengths, 16);
        setup.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    black_box(product_records(
                        black_box(&fixture),
                        black_box(case.options.clone()),
                    ))
                });
            },
        );
    }
    setup.finish();

    let mut iteration = criterion.benchmark_group("core/product/iterate");
    for case in &cases {
        let fixture = lists(&case.lengths, 16);
        let total = checked_product(&case.lengths);
        let expected = total
            .saturating_sub(case.options.offset)
            .min(case.options.limit.unwrap_or(u128::MAX));
        assert_eq!(
            product_records(&fixture, case.options.clone()).count() as u128,
            expected,
            "product fixture count changed for {}",
            case.name
        );
        iteration.throughput(Throughput::Elements(expected as u64));
        iteration.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    black_box(consume_index_vectors(product_records(
                        black_box(&fixture),
                        black_box(case.options.clone()),
                    )))
                });
            },
        );
    }
    iteration.finish();
}

fn zip_and_concat_benchmarks(criterion: &mut Criterion) {
    struct ZipCase {
        name: &'static str,
        lengths: &'static [usize],
        options: ZipOptions,
    }
    let zip_cases = [
        ZipCase {
            name: "small/equal/forward",
            lengths: &[128, 128, 128, 128],
            options: ZipOptions::default(),
        },
        ZipCase {
            name: "medium/unequal/truncate",
            lengths: &[512, 384, 640],
            options: ZipOptions {
                on_unequal: UnequalPolicy::Truncate,
                ..ZipOptions::default()
            },
        },
        ZipCase {
            name: "medium/unequal/cycle-reverse-page",
            lengths: &[512, 384, 640],
            options: ZipOptions {
                on_unequal: UnequalPolicy::Cycle,
                reverse: true,
                offset: 64,
                limit: Some(256),
            },
        },
    ];
    let mut zip_group = criterion.benchmark_group("core/zip/iterate");
    for case in &zip_cases {
        let fixture = lists(case.lengths, 16);
        let expected = zip_records(&fixture, case.options.clone())
            .expect("valid zip fixture")
            .count() as u128;
        assert!(expected > 0);
        zip_group.throughput(Throughput::Elements(expected as u64));
        zip_group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    let records = zip_records(black_box(&fixture), black_box(case.options.clone()))
                        .expect("valid zip fixture");
                    black_box(consume_index_vectors(records))
                });
            },
        );
    }
    zip_group.finish();

    struct ConcatCase {
        name: &'static str,
        lengths: &'static [usize],
        options: ConcatOptions,
    }
    let concat_cases = [
        ConcatCase {
            name: "small/ragged/forward",
            lengths: &[32, 64, 128],
            options: ConcatOptions::default(),
        },
        ConcatCase {
            name: "medium/ragged/reverse-page",
            lengths: &[64, 128, 256, 512],
            options: ConcatOptions {
                reverse: true,
                offset: 32,
                limit: Some(512),
            },
        },
    ];
    let mut concat_group = criterion.benchmark_group("core/concat/iterate");
    for case in &concat_cases {
        let fixture = lists(case.lengths, 16);
        let expected = concat_records(&fixture, case.options.clone())
            .expect("bounded concat fixture")
            .count() as u128;
        assert!(expected > 0);
        concat_group.throughput(Throughput::Elements(expected as u64));
        concat_group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    let mut checksum = 0usize;
                    let mut count = 0u128;
                    for (list, item) in
                        concat_records(black_box(&fixture), black_box(case.options.clone()))
                            .expect("bounded concat fixture")
                    {
                        count = count.checked_add(1).expect("bounded count");
                        checksum = checksum.wrapping_add(list).wrapping_add(item);
                    }
                    black_box((count, checksum))
                });
            },
        );
    }
    concat_group.finish();
}

fn selection_benchmarks(criterion: &mut Criterion) {
    #[derive(Clone, Copy)]
    enum Kind {
        Permutations { n: usize },
        Combinations { n: usize, k: usize },
        Variations { n: usize, k: usize },
    }
    struct Case {
        name: &'static str,
        kind: Kind,
        options: SelectionOptions,
    }
    let cases = [
        Case {
            name: "small/permutations/8",
            kind: Kind::Permutations { n: 8 },
            options: SelectionOptions {
                limit: Some(256),
                ..SelectionOptions::default()
            },
        },
        Case {
            name: "medium/permutations/12/high-offset-reverse",
            kind: Kind::Permutations { n: 12 },
            options: SelectionOptions {
                reverse: true,
                offset: 1_000_000,
                limit: Some(128),
            },
        },
        Case {
            name: "medium/combinations/32-choose-6",
            kind: Kind::Combinations { n: 32, k: 6 },
            options: SelectionOptions {
                limit: Some(256),
                ..SelectionOptions::default()
            },
        },
        Case {
            name: "large-logical/combinations/64-choose-8/high-offset",
            kind: Kind::Combinations { n: 64, k: 8 },
            options: SelectionOptions {
                offset: 10_000_000,
                limit: Some(128),
                ..SelectionOptions::default()
            },
        },
        Case {
            name: "medium/variations/24-choose-4/reverse",
            kind: Kind::Variations { n: 24, k: 4 },
            options: SelectionOptions {
                reverse: true,
                offset: 64,
                limit: Some(256),
            },
        },
    ];

    let mut group = criterion.benchmark_group("core/selection/unrank");
    for case in &cases {
        let expected = case
            .options
            .limit
            .expect("all selection fixtures are bounded");
        let actual = match case.kind {
            Kind::Permutations { n } => permutations(n, case.options)
                .expect("valid permutation fixture")
                .count(),
            Kind::Combinations { n, k } => select_combinations(n, k, case.options)
                .expect("valid combination fixture")
                .count(),
            Kind::Variations { n, k } => variations(n, k, case.options)
                .expect("valid variation fixture")
                .count(),
        } as u128;
        assert_eq!(actual, expected, "selection fixture count changed");
        group.throughput(Throughput::Elements(expected as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    let result = match case.kind {
                        Kind::Permutations { n } => consume_index_vectors(
                            permutations(black_box(n), black_box(case.options))
                                .expect("valid permutation fixture"),
                        ),
                        Kind::Combinations { n, k } => consume_index_vectors(
                            select_combinations(
                                black_box(n),
                                black_box(k),
                                black_box(case.options),
                            )
                            .expect("valid combination fixture"),
                        ),
                        Kind::Variations { n, k } => consume_index_vectors(
                            variations(black_box(n), black_box(k), black_box(case.options))
                                .expect("valid variation fixture"),
                        ),
                    };
                    black_box(result)
                });
            },
        );
    }
    group.finish();
}

fn consume_join(record: combinator_core::JoinedRecord, checksum: &mut u64) {
    for (name, value) in record.fields {
        for byte in name.bytes() {
            *checksum ^= u64::from(byte);
            *checksum = checksum.wrapping_mul(1_099_511_628_211);
        }
        if let Some(value) = value {
            for byte in value.bytes() {
                *checksum ^= u64::from(byte);
                *checksum = checksum.wrapping_mul(1_099_511_628_211);
            }
        }
    }
}

fn join_benchmarks(criterion: &mut Criterion) {
    struct Case {
        name: &'static str,
        left: Vec<combinator_core::Record>,
        right: Vec<combinator_core::Record>,
        kind: JoinType,
        max_fanout: u128,
    }
    let mut cases = Vec::new();
    for kind in [
        JoinType::Inner,
        JoinType::Left,
        JoinType::Full,
        JoinType::Anti,
    ] {
        let right = if kind == JoinType::Anti {
            // Half of the unique left keys remain unmatched so the anti stream
            // exercises both rejection and record construction.
            join_records("right", 256, 256, "key-")
        } else {
            join_records("right", 512, 512, "key-")
        };
        cases.push(Case {
            name: match kind {
                JoinType::Inner => "medium/unique/inner",
                JoinType::Left => "medium/unique/left",
                JoinType::Full => "medium/unique/full",
                JoinType::Anti => "medium/unique/anti",
            },
            left: join_records("left", 512, 512, "key-"),
            right,
            kind,
            max_fanout: 1,
        });
    }
    cases.extend([
        Case {
            name: "medium/no-matches/full",
            left: join_records("left", 512, 512, "left-key-"),
            right: join_records("right", 512, 512, "right-key-"),
            kind: JoinType::Full,
            max_fanout: 1,
        },
        Case {
            name: "medium/skewed-duplicates/inner",
            left: join_records("left", 256, 64, "key-"),
            right: join_records("right", 256, 64, "key-"),
            kind: JoinType::Inner,
            max_fanout: 16,
        },
        Case {
            name: "medium/fanout-at-limit/inner",
            left: join_records("left", 32, 1, "key-"),
            right: join_records("right", 32, 1, "key-"),
            kind: JoinType::Inner,
            max_fanout: 1_024,
        },
        Case {
            name: "medium/long-common-prefix/inner",
            left: join_records("left", 512, 512, &"x".repeat(128)),
            right: join_records("right", 512, 512, &"x".repeat(128)),
            kind: JoinType::Inner,
            max_fanout: 1,
        },
    ]);

    let mut count_group = criterion.benchmark_group("core/join/count");
    for case in &cases {
        let expected = join_count_with_fanout(
            &case.left,
            &case.right,
            "id",
            "id",
            case.kind,
            10_000,
            case.max_fanout,
        )
        .expect("valid bounded join fixture");
        count_group.throughput(Throughput::Elements(
            u64::try_from(case.left.len() + case.right.len()).expect("bounded input size"),
        ));
        count_group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    black_box(
                        join_count_with_fanout(
                            black_box(&case.left),
                            black_box(&case.right),
                            "id",
                            "id",
                            black_box(case.kind),
                            10_000,
                            case.max_fanout,
                        )
                        .expect("valid bounded join fixture"),
                    )
                });
            },
        );
        assert!(expected <= 10_000);
    }
    count_group.finish();

    let mut stream_group = criterion.benchmark_group("core/join/stream");
    for case in &cases {
        let expected = join_count_with_fanout(
            &case.left,
            &case.right,
            "id",
            "id",
            case.kind,
            10_000,
            case.max_fanout,
        )
        .expect("valid bounded join fixture");
        stream_group.throughput(Throughput::Elements(expected as u64));
        stream_group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            case,
            |bencher, case| {
                bencher.iter(|| {
                    let mut checksum = 0u64;
                    let selected = join_each_with_fanout(
                        black_box(&case.left),
                        black_box(&case.right),
                        "id",
                        "id",
                        black_box(case.kind),
                        0,
                        Some(expected),
                        10_000,
                        case.max_fanout,
                        None,
                        |record| {
                            consume_join(record, &mut checksum);
                            Ok::<(), CoreError>(())
                        },
                    )
                    .expect("valid bounded join fixture");
                    black_box((selected, checksum))
                });
            },
        );
    }
    stream_group.finish();
}

criterion_group! {
    name = benches;
    config = config();
    targets = product_benchmarks, zip_and_concat_benchmarks, selection_benchmarks, join_benchmarks
}
criterion_main!(benches);
