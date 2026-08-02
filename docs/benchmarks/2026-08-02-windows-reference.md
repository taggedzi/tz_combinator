# Windows reference baseline — 2026-08-02

This is the first complete reference run of the bounded performance suite. It
is orientation evidence, not a performance guarantee or regression gate. The
saved local Criterion baseline was named
`windows-reference-2026-08-02-v2`; generated samples remain ignored below
`target/criterion` and are not committed.

## Reproduction context

| Property | Value |
|---|---|
| Repository base commit | `e16eff1c538901a9afcbc4e9d0425ed283a2e93c` |
| Benchmark suite | Initial implementation in the same changeset as this record |
| Rust | `rustc 1.94.1 (e408947bf 2026-03-25)` |
| Target | `x86_64-pc-windows-msvc` |
| OS | Microsoft Windows NT 10.0.26200.0 |
| CPU | 12th Gen Intel Core i7-12700KF; 20 logical processors visible |
| Cargo profile | Workspace release/bench profile: opt-level 3, thin LTO, one codegen unit |
| Criterion bounds | 20 samples, 250 ms warm-up, 750 ms measurement; 10 samples and one-second measurement for CLI; 10 samples for application file sinks |
| Complete command wall time | 86.4 seconds |

The power policy and unrelated host load were not captured reliably, so future
comparisons must use the workflow metadata and should confirm material changes
with a fresh controlled-host run. The benchmark names, fixture sizes, build
prerequisites, and exact command are documented in the
[benchmarking guide](../benchmarking.md).

The release CLI was built first, then the baseline was recorded with:

```text
cargo build -p combinator-cli --release --locked
cargo bench -p combinator-benchmarks --locked -- \
  --noplot --save-baseline windows-reference-2026-08-02-v2
```

## Representative medians

Criterion estimates are in elapsed time per complete benchmark iteration. The
interval is the 95% bootstrap confidence interval for the median. CV is the
sample standard deviation divided by the mean and is included as a compact
variability signal.

| Benchmark | Median | 95% median interval | CV |
|---|---:|---:|---:|
| CLI version startup | 4.679 ms | 4.651–4.791 ms | 4.30% |
| CLI 1,024-record product | 5.561 ms | 5.523–5.623 ms | 1.22% |
| Parse 2,048 CSV values | 298.744 µs | 297.204–299.837 µs | 1.83% |
| Render 16 wide JSONL fields | 4.232 µs | 4.215–4.245 µs | 1.21% |
| Application text to counting sink (512) | 181.638 µs | 180.901–182.690 µs | 1.94% |
| Application JSONL to counting sink (512) | 315.884 µs | 314.915–317.787 µs | 0.93% |
| Application create-new file (512) | 2.845 ms | 2.784–2.890 ms | 2.12% |
| Application safe replacement (512) | 3.404 ms | 3.326–3.415 ms | 1.29% |
| Product, two fields, 128-record page | 3.598 µs | 3.587–3.620 µs | 1.04% |
| Product, 12-field high offset, 256-record page | 7.619 µs | 7.562–7.640 µs | 1.35% |
| Permutations of 8, 256-record page | 35.609 µs | 35.476–35.791 µs | 0.74% |
| 64 choose 8 at high offset, 128-record page | 61.191 µs | 60.918–61.443 µs | 1.10% |
| Unique inner join count, 512×512 | 108.719 µs | 107.765–109.611 µs | 1.80% |
| Long-common-prefix inner join count, 512×512 | 214.715 µs | 213.535–215.553 µs | 1.16% |
| Unique inner join stream, 512 records | 637.556 µs | 633.406–640.681 µs | 1.96% |
| Skewed-duplicate join stream, 1,024 records | 1.089 ms | 1.083–1.093 ms | 2.73% |

The complete saved baseline contains every workload in the guide; this table is
intentionally concise and does not replace the raw samples or an opt-in HTML
comparison report.

## Reliability notes

- The CLI startup case had the broadest representative variation (4.30% CV)
  and should be repeated before attributing a small startup change to code.
- The small equal-length zip case classified 30% of samples as outliers even
  though its central interval was narrow. Treat it as unreliable until repeated
  runs show whether the distribution is stable.
- Reverse permutations of 12 and the skewed/fanout join-stream cases each
  classified 25% of samples as outliers. They remain useful diagnostic cases,
  but a single comparison is insufficient for a release decision.
- Shared CI runners are expected to be noisier. CI comparisons are signals for
  controlled-host confirmation and have no timing pass/fail threshold.

## Initial interpretation

- Process startup accounts for most of a tiny CLI invocation: the four-record
  product is about 0.6 ms slower than `--version`. Core iterator optimization
  alone is unlikely to materially change that workflow.
- At the application layer, JSONL serialization is about 1.7× the text counting
  path, while durable file commit is an order of magnitude slower than either
  in-memory path. CPU algorithms, serialization, and filesystem work must stay
  separate in future comparisons.
- High product offsets remain constant-time to resolve at this scale: a
  999,999,999,500 offset changes bounded-page work by microseconds, not by the
  logical prefix length.
- Selection unranking is materially costlier per output than product or zip,
  consistent with its per-record rank/unrank work. This is evidence for focused
  profiling, not yet evidence for a particular implementation change.
- Join streaming takes substantially longer than join counting, and
  long-common-prefix keys roughly double unique join-count time. Joined-record
  construction/cloning and key hashing are the clearest profiling candidates.

No production optimization was made from this baseline. A future optimization
must first reproduce its target case, profile the relevant phase where useful,
and preserve all correctness and resource-limit tests.

## Report-path validation

A full same-worktree comparison against this baseline successfully generated
`target/criterion/report/index.html` and its local SVG assets. It took 348.9
seconds and produced a 15.4 MB Criterion tree that also contained earlier test
baselines; both remain below the workflow's explicit bounds.

Despite no source change, the comparison classified several cases as improved
and one small NUL parse case as regressed; one wide CSV parse result moved by
roughly 41%. This is direct evidence of cache, host-state, and run-order effects,
not an optimization result. It validates the policy that shared or uncontrolled
timing changes are never automatic release gates and must be reproduced on a
controlled host before engineering conclusions are drawn.
