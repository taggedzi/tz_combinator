# Benchmarking guide

This guide describes the reproducible, bounded performance suite for
`tz_combinator`. Benchmark results are evidence for maintainers, not performance
guarantees. Correctness, resource limits, output ordering, and filesystem safety
remain authoritative when a faster implementation disagrees with a test.

## Framework and licensing

The non-published `combinator-benchmarks` workspace package owns the benchmark
harness. Production crates do not depend on its tools.

Criterion 0.8.2 is the selected framework because it supports the pinned Rust
toolchain, stable Windows and Linux, bounded warm-up and measurement durations,
throughput measurements, named baselines, statistical comparisons, and optional
HTML reports. Its license is `MIT OR Apache-2.0`; this project uses it under the
MIT option. Plotters 0.3.7 is enabled only by the `benchmark-report` feature and
is MIT licensed. Tempfile 3.27.0 is `MIT OR Apache-2.0` and is used only to give
file benchmarks dedicated temporary directories.

The unstable built-in Rust benchmark interface was rejected because this
workspace supports stable Rust. Iai/Callgrind-style tools were not selected for
the portable harness because their external profiler requirements do not cover
the supported Windows and Linux paths uniformly. Other stable harnesses were
not selected because Criterion already supplies the required saved-baseline and
readable-report workflow.

Run the complete dependency policy, including benchmark and report features,
after changing this stack:

```text
cargo deny --all-features check
```

`deny.toml` includes development dependencies. Also review non-Cargo actions,
scripts, templates, and assets manually because Cargo metadata cannot license
check them. See [dependency licensing](dependency-licenses.md).

## Prerequisites

Use the committed lockfile and the Rust version in `rust-toolchain.toml`. Close
unrelated CPU-, memory-, and disk-intensive applications. Disable power-saving
or thermal-throttling modes where practical. Build the release CLI before a run
that includes the CLI macrobenchmarks:

```text
cargo build -p combinator-cli --release --locked
cargo bench -p combinator-benchmarks --no-run --locked
```

The CLI benchmark finds `target/release/combinator` (or `combinator.exe`). Set
`COMBINATOR_BENCH_BIN` only when intentionally measuring another release binary.

## Exact fixture conventions

Fixtures are deterministic synthetic ASCII strings. They contain no user data,
paths, secrets, or ambient configuration. The shared size labels are:

| Label | Logical records | Payload shape |
|---|---:|---|
| `small` | 128 | Narrow values, normally at least 16 or 24 bytes |
| `medium` | 2,048 | Narrow values or explicitly bounded pages |
| `large` | 8,192 | Logical label; large spaces are always windowed |

The implemented cases are deliberately more precise than those broad labels:

| Area | Bounded cases |
|---|---|
| Product | 2 fields × 16 items (128-record page); 8 fields × 4 items (256); 32 fields × 2 items, reverse (128); ragged 2×3×5×7 (210); 12 fields × 10 items at offset 999,999,999,500 (256) |
| Zip | Four equal 128-item lists; unequal 512/384/640 lists with truncate; the same unequal lists with cycle, reverse, offset 64, and limit 256 |
| Concat | Ragged 32/64/128 lists; ragged 64/128/256/512 lists with reverse paging limited to 512 |
| Selection | Permutations of 8 (256); reverse permutations of 12 after offset 1,000,000 (128); 32 choose 6 (256); 64 choose 8 after offset 10,000,000 (128); reverse 24P4 (256) |
| Join | 512×512 unique keys for every join kind; no matches; 256×256 keys with four duplicates per side; 32×32 fanout exactly at 1,024; 512 long-common-prefix keys |
| Codec parse | 128×24-byte, 2,048×24-byte, and 512×256-byte equivalent values in lines, CSV, TSV, and NUL formats |
| Codec render | Four narrow fields and sixteen 256-byte escaping-heavy fields in text, CSV, TSV, JSONL, and NUL formats |
| Application | 512 rendered product records to a consuming counting sink and to create-new/safe-replacement file sinks |
| CLI | Version startup; four-record product; 1,024-record product; 512-record CSV-to-JSONL; 1,024-record create-new file output |

Every case asserts its expected count before timing. Iterators, rendered bytes,
join fields, subprocess output, and file output are consumed so the optimizer
cannot discard the measured work. Logical spaces larger than a benchmark page
are never materialized.

## Ephemeral and smoke runs

The normal command prints concise statistics, disables plots, and discards all
samples. It creates no persistent baseline or report:

```text
cargo build -p combinator-cli --release --locked
cargo bench -p combinator-benchmarks --locked -- --noplot --discard-baseline
```

Run only one layer with `--bench core`, `--bench codec_app`, or `--bench cli`.
Use Criterion's test mode to execute every fixture once without timing it:

```text
cargo bench -p combinator-benchmarks --locked -- --test
```

The harness uses 20 samples, a 250 ms warm-up, and a 750 ms target measurement
per library benchmark. Filesystem cases use 10 samples. CLI cases use 10 samples
and a one-second target measurement. A complete run normally takes several
minutes; hardware and filesystem behavior dominate the exact duration. On the
initial Windows reference host, the no-plot suite took about 86 seconds, while a
full opt-in comparison with Plotters reports took about 349 seconds. Report
generation is therefore intentionally not part of routine commands.

## Save and compare a baseline

Saving data is explicit. Use a short deterministic name containing only ASCII
letters, digits, `.`, `_`, or `-`; include the reason rather than a hostname or
path. For example:

```text
cargo bench -p combinator-benchmarks --locked -- \
  --noplot --save-baseline before-serde-update
```

The baseline is stored below the ignored `target/criterion` directory. To make
a statistically evaluated comparison and generate the readable report:

```text
cargo bench -p combinator-benchmarks --features benchmark-report --locked -- \
  --baseline before-serde-update
```

Open `target/criterion/report/index.html`. The report shows estimates,
uncertainty, distributions, and changes from the named baseline. Keep the raw
Criterion directories with the HTML when another maintainer must independently
inspect the comparison.

Retain no more than eight local named baselines. Remove an obsolete baseline
only after confirming its exact directory name below each benchmark's
`target/criterion` tree; never use a broad or unresolved deletion target. CI
artifacts use a separate one-baseline-per-artifact policy and expire after 14
days.

Compare only runs from the same host, target, Rust toolchain, power policy, and
benchmark configuration. A dependency update itself is a valid changed input;
changing the host at the same time is not. Treat shared-runner results as a
signal to reproduce on a controlled host, never as a release gate.

## Manual CI artifact

The **Performance benchmarks** GitHub Actions workflow is manual only. Its
operations are:

- `discard`: run without retaining samples or uploading anything;
- `save`: save a named baseline and upload it for 14 days;
- `compare`: download the same named baseline from the supplied workflow run
  ID and compare the current checkout with it.

Set `report` only with `compare`. The conditional artifact contains
`OPEN_ME.html`, the Criterion report, bounded raw comparison data, current and
baseline metadata, and a comparability warning. Baseline names and run IDs are
validated, report output is capped at 100 MiB, and no artifact step runs unless
the dispatch inputs request one.

The recorded metadata is limited to commit, Rust version, target, runner OS,
runner architecture/image, CPU model and visible processor count, baseline
name, and benchmark operation. It does not record the hostname, user paths,
environment dump, or fixture content.
The initial complete local report tree, including several test baselines, was
about 15.4 MB; CI still fails closed at the documented 100 MiB limit.

## Interpreting and recording results

Use the median estimate and confidence interval rather than the fastest sample.
Record variance and Criterion's change classification. Investigate a change
only when it repeats on the same controlled host and is large enough to matter
to a representative user workflow. Separate CPU, allocation, serialization,
and filesystem evidence before changing code.

For an optimization review, record:

1. commit and named baseline;
2. toolchain, target, sanitized host characteristics, and power policy;
3. exact benchmark names and command;
4. before/after estimates, percentage change, and uncertainty;
5. correctness tests and resource-limit checks run;
6. neutral or negative results as well as improvements.

Optional allocation or platform profiling is outside the portable harness. Use
only trusted, locally installed tools and only on the bounded representative
cases. Do not add a profiler, network dependency, unsafe path, or CI requirement
to the normal benchmark suite merely to collect optional evidence.

The initial complete run and its reliability notes are recorded in the
[2026-08-02 Windows reference baseline](benchmarks/2026-08-02-windows-reference.md).
