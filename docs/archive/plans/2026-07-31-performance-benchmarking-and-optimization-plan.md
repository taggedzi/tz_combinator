# Performance Benchmarking and Optimization Plan

> Proposed implementation plan dated 2026-07-31. This document does not
> describe implemented behavior. Repository code, current manuals, and the
> compatibility policy remain authoritative until individual tasks land.

## Objective

Establish repeatable performance evidence for `tz_combinator`, use profiling
to identify material bottlenecks, and make narrowly scoped optimizations
without weakening resource limits, deterministic ordering, output safety, or
CLI compatibility.

The long-term optimization goal is broad general-user performance across the
platforms and architectures the project supports, not peak performance on one
AMD64/Windows host. Portable release builds must remain the default and should
be efficient without local CPU-specific compilation. Experienced users may
opt into native builds for additional local performance. Any target-specific
fast path must retain a portable fallback, preserve the same safety and
compatibility contracts, and be evaluated on the affected targets.

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
- an explicitly requested, readable comparison report that can be downloaded
  as a CI artifact without becoming part of normal benchmark runs;
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
- A result from one operating-system and architecture combination must not be
  treated as universal evidence. Optimization decisions must identify the
  representative platform/architecture set they are intended to improve and
  check for unacceptable regressions on the other supported combinations.
- Routine and smoke runs must not generate plots, persistent reports, saved
  baselines, or uploaded artifacts. Each of those outputs requires an explicit
  request.
- Saved results and report artifacts must have bounded retention and must not
  contain raw fixture data, user paths, hostnames, secrets, or other sensitive
  environment values.
- Benchmark dependencies, reporting tools, bundled report assets, and their
  transitive dependencies must not require changing the workspace's MIT
  license.
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
- use licenses compatible with retaining the workspace's MIT license and pass
  the project's license and source policy, including the development
  dependency graph;
- permit deterministic benchmark names and bounded sample durations.

Record the choice and rejected alternatives in the benchmark guide. Add the
framework only as a development dependency of packages that own benchmarks.
Do not enable report-generation features by default when the framework permits
them to be selected independently.

## Licensing and opt-in reports

The workspace and all first-party benchmark code remain MIT licensed. Adding a
benchmark or reporting dependency must not require relicensing the project.
Before accepting any such dependency or generated report format:

1. Inspect the exact locked direct and transitive dependency graph, including
   development dependencies and optional report features.
2. Run the repository's license/source checks with development dependencies
   included. Configure `cargo-deny` accordingly rather than relying on its
   default treatment of development dependencies.
3. Review non-Cargo tools, GitHub Actions, templates, JavaScript, stylesheets,
   fonts, and other assets included in or distributed with a report; these are
   outside the Rust dependency graph.
4. Update third-party notices when required. Reject or replace any component
   whose terms would require changing the workspace license or impose an
   incompatible distribution requirement.

Routine benchmark and CI smoke commands produce only concise console output and
discard their samples after the run. Persistent output is opt-in and has two
separate operations:

- **Save baseline:** store compact, machine-readable samples under an explicit,
  deterministic baseline name for a later comparison.
- **Generate comparison report:** compare the current run with a named baseline
  and create a self-contained, human-readable HTML report plus the compact raw
  comparison data needed for independent inspection.

An opt-in report shows current and baseline values, absolute and percentage
changes, uncertainty or variance, and unreliable cases. It also records the
commit, Rust toolchain, target, benchmark configuration, and sanitized host
characteristics needed to judge comparability. It must prominently warn when
the current and baseline environments differ materially. Report generation
must not rerun benchmarks solely to render data that the requested comparison
already collected.

Local reports remain in ignored build/output directories. CI exposes reports
only through a manually dispatched workflow with an explicit report request and
named baseline; the upload step is conditional on that request. Artifacts are
compressed, use a documented finite retention period, and are never attached to
normal builds or releases automatically.

Acceptance criteria:

- Adding the benchmark and optional reporting stack does not change the MIT
  license of any workspace crate or first-party source file.
- License checks cover all features used to create the distributed report and
  fail closed on an unapproved, unknown, or incompatible license or source.
- A routine benchmark or CI smoke run creates no persistent report, saved
  baseline, or uploaded artifact.
- One documented opt-in command can save a named baseline, and one documented
  opt-in command or manual workflow can produce a readable comparison artifact.
- Report size, retained baseline count, CI artifact retention, sample duration,
  and benchmark output remain explicitly bounded.

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
   baseline naming, sanitized host metadata, expected runtime, routine
   no-report operation, opt-in report generation, artifact retention, and
   result comparison.
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
- The guide distinguishes ephemeral runs, saved baselines, and generated
  comparison reports and gives each an explicit command.

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
4. When explicitly requested, generate a readable comparison report from the
   current results and a named baseline without repeating completed benchmark
   work.
5. Profile only the slow or allocation-heavy representative cases using tools
   available on the relevant platform.
6. Attribute costs to phases before proposing code changes. For example,
   distinguish iterator allocation from serialization and filesystem writes.
7. Rank optimization candidates by user impact, measured cost, implementation
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
- Requested comparison reports are readable without specialized benchmark
  tooling and preserve the underlying bounded comparison data.

## Phase 5: Optimization workflow

For each confirmed bottleneck:

1. Define the exact benchmark, expected improvement, compatibility contract,
   and memory/resource tradeoff.
2. Add or identify correctness and hostile-input tests that protect the code
   path.
3. Make one coherent optimization at a time without unrelated refactoring.
4. Run the focused benchmark before and after on the same host and toolchain.
5. Repeat the focused comparison on at least one representative target from
   each affected operating-system and architecture family. For target-specific
   code, test both the fast path and the portable fallback; a Windows x86-64
   improvement alone is not sufficient to establish a general optimization.
6. Run focused tests, then the full workspace verification appropriate to the
   affected trust boundary.
7. Review the diff for unchecked arithmetic, accidental materialization,
   cancellation latency, changed ordering, and new attacker-controlled memory
   growth.
8. Keep the change only when the improvement is repeatable and worth its
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
- offer a manually dispatched workflow whose explicit inputs select whether to
  save a named baseline and generate a readable comparison artifact;
- skip report rendering and artifact upload unless those inputs request them;
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
cargo deny --all-features check
```

Run the selected benchmark targets in release mode after they compile. Each
optimization additionally requires the focused before/after benchmark and the
normal workspace checks. Security-sensitive filesystem changes require
targeted integration tests and platform-specific review. The license check must
include development dependencies and every optional feature used to generate a
distributed report.

## Completion criteria

- The workload matrix is implemented and documented.
- A reproducible baseline exists for a documented reference environment.
- An explicitly requested, readable current-versus-baseline report can be
  downloaded as a bounded-retention artifact, while routine runs create none.
- Benchmark targets compile under the MSRV with locked dependencies.
- Benchmark and report dependencies pass the license/source policy without
  changing the workspace's MIT license.
- Normal CI verifies benchmark code without relying on noisy timing gates.
- Each accepted optimization has before/after evidence and correctness tests.
- Portable release performance remains acceptable across the supported target
  matrix, while any native-only tuning is documented as an opt-in build or
  runtime-selected fast path with a tested fallback.
- Resource ceilings, deterministic output, CLI contracts, and filesystem
  safety remain unchanged unless separately approved and documented.
