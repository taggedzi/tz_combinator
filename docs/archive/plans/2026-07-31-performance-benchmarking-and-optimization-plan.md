# Performance Benchmarking and Optimization Plan

> Proposed implementation plan dated 2026-07-31. This document does not
> describe implemented behavior. Repository code, current manuals, and the
> compatibility policy remain authoritative until individual tasks land.

## Objective

Establish repeatable performance evidence for `tz_combinator`, use profiling
to identify material bottlenecks, and make narrowly scoped optimizations
without weakening resource limits, deterministic ordering, output safety, or
CLI compatibility.

The first deliverable is measurement infrastructure and a recorded baseline.
Optimization work begins only after a benchmark or profile demonstrates a
meaningful bottleneck.

## Scope

This plan covers:

- microbenchmarks for the core iterators, selection algorithms, and joins;
- throughput benchmarks for parsing, rendering, and application streaming;
- a small set of release-mode CLI macrobenchmarks;
- optional allocation and memory profiling outside the normal test suite;
- evidence-driven optimization of confirmed hot paths;
- documentation of benchmark methods and interpretation;
- optional CI collection of benchmark results without an initially flaky
  timing gate.

This plan does not authorize changing output order, error codes, limits,
filesystem semantics, the MSRV, or the supported CLI. It also does not make
unbounded stress tests part of normal CI.

## Required invariants

- Benchmarks must consume their results so the optimizer cannot remove the
  work being measured.
- Fixtures must be deterministic, bounded, synthetic, and free of secrets or
  personal data.
- Large logical spaces must be tested through bounded windows; benchmarks
  must not accidentally materialize combinatorial output.
- Benchmark-only code must not create a production path around hard resource
  ceilings.
- All optimized implementations must preserve exact output, ordering, checked
  arithmetic, cancellation behavior, and failure semantics.
- File benchmarks must use dedicated temporary directories and must not
  follow attacker-controlled links or overwrite unrelated files.
- Performance measurements must use release settings and record enough host
  context to make comparisons honest.
- Normal correctness and security tests remain authoritative. A faster result
  that changes behavior is a regression.

## Measurement strategy

Use three layers of measurement because they answer different questions:

1. **Core microbenchmarks** isolate algorithm and allocation costs without
   parsing, serialization, process startup, or filesystem noise.
2. **Codec and application benchmarks** measure realistic parse-render-stream
   paths with an in-memory sink and, separately, a temporary file sink.
3. **CLI macrobenchmarks** measure a few representative release-binary
   invocations. These are useful for user-visible latency but should not be
   used to diagnose an algorithm by themselves.

For each benchmark, report either latency per operation or throughput in
records/bytes per second. Measure peak memory or allocation counts separately
where tooling and platform support are reliable; do not mix optional profiler
requirements into the portable benchmark harness.

## Benchmark framework decision

Before adding dependencies, compare stable Rust benchmark frameworks against
the declared MSRV and the locked dependency policy. Criterion is the initial
candidate because it supports statistical comparisons, warm-up, throughput,
and saved baselines. The selected version must:

- support Rust 1.94.1 and Rust 2021;
- run on supported Windows and Linux targets;
- work with `cargo bench --locked`;
- avoid production dependencies;
- permit deterministic benchmark names and bounded sample durations.

Record the choice and rejected alternatives in the benchmark guide. Add the
framework only as a development dependency of packages that own benchmarks.

## Representative workload matrix

Keep the initial matrix small enough to run locally while covering distinct
algorithmic behavior.

| Area | Cases | Primary signal |
|---|---|---|
| Product | 2, 8, and 32 fields; short and ragged lists; forward/reverse; small and very large offsets; bounded limits | tuples/second and latency |
| Zip/concat | equal and unequal lengths; forward/reverse; bounded paging | records/second |
| Selection | permutations, combinations, and variations at small, medium, and near-practical bounded sizes; forward/reverse pages | items/second and allocations |
| Join count | inner/left/full/anti; mostly unique keys; no matches; skewed duplicates; fanout near the configured ceiling | input records/second and memory |
| Join stream | the same distributions with bounded output pages and an in-memory sink | output records/second |
| Codecs | lines, CSV, TSV, and JSONL; narrow and wide records; escaping-heavy data | input/output bytes per second |
| Application | generate plus render to a counting sink; generate plus temporary file output | records/second and bytes/second |
| CLI | startup/small input, medium in-memory generation, codec-heavy input, and temporary file output | wall-clock time and peak memory |

Include at least one adversarial-but-valid case per relevant area, such as
collision-heavy join keys or escaping-heavy records. Keep these cases below
hard resource ceilings and separate them from malformed-input correctness
tests.

## Phase 1: Benchmark conventions and fixtures

1. Add a benchmark guide describing prerequisites, release-mode commands,
   baseline naming, host metadata, expected runtime, and result comparison.
2. Define deterministic fixture builders instead of committing large generated
   datasets.
3. Define size labels such as `small`, `medium`, and `large` with exact item,
   field, record, and byte counts.
4. Define a no-output or counting sink for application benchmarks that still
   performs all required serialization and limit accounting.
5. Keep filesystem fixtures in a per-run temporary directory and document
   cleanup behavior.
6. Establish benchmark naming that includes the operation and workload shape,
   but never includes raw input values or paths.

Acceptance criteria:

- A new contributor can reproduce the benchmark suite from the guide.
- Fixture generation is deterministic and bounded.
- Running benchmarks does not modify tracked files unless the user explicitly
  requests saving a baseline.
- Benchmark results identify the toolchain, target, profile, and relevant host
  characteristics.

## Phase 2: Core benchmarks

1. Add product benchmarks that distinguish iterator setup, offset resolution,
   and bounded iteration.
2. Add zip and concat benchmarks for normal and edge-shaped input lengths.
3. Add selection benchmarks that consume generated index vectors and expose
   the cost of rank/unrank operations.
4. Add join-count benchmarks for key indexing, duplicate validation, and each
   join kind.
5. Add streaming-join benchmarks using a callback that consumes every field
   without retaining the complete output.
6. Assert fixture counts before timing so a mistaken workload cannot silently
   benchmark the wrong operation.

Acceptance criteria:

- Every major core operation has at least one small and one medium benchmark.
- Large logical counts are measured through explicit limits.
- Forward/reverse and high-offset paths are represented where supported.
- Benchmark code contains no unchecked arithmetic derived from fixture sizes.

## Phase 3: Codec and application benchmarks

1. Benchmark supported input parsers with equivalent logical records and
   format-specific escaping cases.
2. Benchmark output rendering independently from file I/O.
3. Benchmark application streaming through an in-memory counting sink.
4. Benchmark file output separately, including create-new and safe replacement
   paths where a stable comparison is possible.
5. Separate parsing, generation, rendering, and I/O measurements so a result
   can identify the dominant phase.
6. Add a few release-binary macrobenchmarks after the library-level suite is
   stable.

Acceptance criteria:

- Codec benchmarks consume equivalent bounded content across formats.
- File benchmarks never write outside their dedicated temporary directory.
- CLI benchmarks verify exit status and output counts before accepting a
  timing sample.
- No benchmark depends on network access or ambient user configuration.

## Phase 4: Baseline and profiling

1. Run the complete suite on a documented reference host with the pinned Rust
   toolchain and committed lockfile.
2. Repeat representative cases enough times to distinguish stable signals
   from host noise.
3. Record a concise baseline summary, including medians, variance, and any
   unreliable cases.
4. Profile only the slow or allocation-heavy representative cases using tools
   available on the relevant platform.
5. Attribute costs to phases before proposing code changes. For example,
   distinguish iterator allocation from serialization and filesystem writes.
6. Rank optimization candidates by user impact, measured cost, implementation
   risk, and effect on memory/resource predictability.

Initial code-review hypotheses to validate, not assumptions to optimize:

- product iteration clones an index tuple for each output;
- selection unranking allocates an available-index vector and removes elements
  for each item;
- join field lookup scans record fields linearly;
- join output construction clones fields and rebuilds collision-name sets;
- serialization or filesystem output may dominate all algorithmic costs.

Acceptance criteria:

- Every proposed optimization cites a reproducible benchmark and, when useful,
  a profile.
- Results distinguish CPU, allocation/memory, serialization, and I/O costs.
- No optimization task is opened solely from source inspection.

## Phase 5: Optimization workflow

For each confirmed bottleneck:

1. Define the exact benchmark, expected improvement, compatibility contract,
   and memory/resource tradeoff.
2. Add or identify correctness and hostile-input tests that protect the code
   path.
3. Make one coherent optimization at a time without unrelated refactoring.
4. Run the focused benchmark before and after on the same host and toolchain.
5. Run focused tests, then the full workspace verification appropriate to the
   affected trust boundary.
6. Review the diff for unchecked arithmetic, accidental materialization,
   cancellation latency, changed ordering, and new attacker-controlled memory
   growth.
7. Keep the change only when the improvement is repeatable and worth its
   complexity. Record neutral or negative findings so they are not repeatedly
   rediscovered.

An optimization is complete only when it preserves behavior, improves the
target metric meaningfully, does not introduce an unacceptable regression in
another representative case, and has benchmark evidence attached to its
review.

## CI and regression policy

Timing-sensitive pass/fail thresholds are intentionally deferred. Shared CI
runners are noisy, and premature thresholds create false failures.

Initial CI scope:

- compile benchmark targets with the pinned toolchain and locked dependencies;
- optionally run a short benchmark smoke subset to catch panics and fixture
  errors;
- offer a manual or scheduled workflow that stores full benchmark artifacts;
- never upload fixture content that could contain user data.

After enough history exists, consider a regression threshold only for cases
that are stable across runners. Require confirmation on a controlled host
before treating a timing change as a release blocker.

## Documentation impact

Implementation should add a maintainer-facing benchmark guide and link it from
the documentation guide or contributing guide. User documentation needs an
update only if an optimization changes documented performance expectations;
it must not imply guarantees that the project cannot test reliably.

## Verification

During benchmark infrastructure work, use:

```text
cargo bench --workspace --no-run --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Run the selected benchmark targets in release mode after they compile. Each
optimization additionally requires the focused before/after benchmark and the
normal workspace checks. Security-sensitive filesystem changes require
targeted integration tests and platform-specific review.

## Completion criteria

- The workload matrix is implemented and documented.
- A reproducible baseline exists for a documented reference environment.
- Benchmark targets compile under the MSRV with locked dependencies.
- Normal CI verifies benchmark code without relying on noisy timing gates.
- Each accepted optimization has before/after evidence and correctness tests.
- Resource ceilings, deterministic output, CLI contracts, and filesystem
  safety remain unchanged unless separately approved and documented.

