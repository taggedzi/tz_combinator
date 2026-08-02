# Optimization evidence — 2026-08-02

This note records the first optimization pass after the Windows reference
baseline. Measurements were taken on the same documented Windows host and
pinned Rust toolchain with the locked release benchmark configuration. They
are evidence for the affected cases, not a portable performance guarantee.

## Accepted changes

Selection unranking now precomputes factorial/falling-factorial rank blocks and
reuses the available-index workspace for permutations and variations. The
returned `Vec<usize>` remains newly owned for every item, so iterator ordering,
item ownership, and resource behavior are unchanged.

The permutation benchmark improved from the recorded 35.609 µs median to
21.434 µs:

| Case | Before | After | Result |
|---|---:|---:|---:|
| permutations of 8, 256-item page | 35.609 µs | 21.434 µs | 39.8% lower |

Joined-record field-name collision mappings are prepared only when a right-side
key expands to multiple outputs. This removes repeated collision-set work from
the confirmed duplicate-heavy stream case while avoiding setup work for
one-to-one joins. The skewed duplicate stream measured about 643 µs after the
change versus the 1.089 ms reference median, an improvement of approximately
40.9%. The result should be repeated on a controlled host before being used as
a release threshold; the reference case had 2.73% CV.

## Rejected candidate

An adjacent-binomial recurrence was tested for combination unranking. The
64-choose-8 high-offset case moved from the 61.191 µs reference median to
67.741 µs, so that change was removed. The existing combination algorithm and
its behavior remain unchanged.

A schema-aware direct key-position accessor was also tested for joins. It
retained a scan fallback for varying schemas, but repeated join-count runs
varied by about the same 1–2% as the apparent gain and Criterion classified
most cases as unchanged. It was removed rather than adding complexity without
repeatable evidence.

## Reproduction

Run the focused release measurements without saving a baseline or report:

```text
cargo bench -p combinator-benchmarks --bench core --locked -- --noplot "core/selection/unrank"
cargo bench -p combinator-benchmarks --bench core --locked -- --noplot "core/join/stream"
```

Correctness coverage for the optimized paths includes the core unit and
boundary tests, exhaustive small combination-order checks, collision-preserving
full-join tests, and the existing cancellation, limit, fanout, and ordering
tests. The normal Linux and Windows quality jobs now also compile every locked
benchmark target. Timing results from other operating systems and architectures
remain required before making a cross-platform performance claim.

The [optimization benchmark matrix workflow](../../.github/workflows/optimization-benchmarks.yml)
provides the manual Linux/Windows timing run for that follow-up. It intentionally
does not save baselines, render reports, or upload artifacts.
