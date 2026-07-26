# Public-Service Security Hardening Plan

This plan addresses the residual risks identified during the hostile-environment review. The goal is to make the CLI/library safe to invoke from untrusted clients, including concurrent or publicly exposed service wrappers.

The implementation should remain narrowly scoped, preserve stable CLI error codes, and keep the existing product/zip/concat behavior unchanged unless explicitly noted.

## Security objectives

- Every input source, including join stdin, is bounded before excess data is retained.
- Join execution cannot materialize an attacker-controlled result set in memory.
- `--offset`, `--limit`, and output-byte limits reduce work during generation rather than only after generation.
- Service callers can cancel requests and enforce wall-clock, CPU, memory, and concurrency limits.
- Output paths remain safe under hostile filesystem changes as far as the platform permits.
- Existing hard ceilings, atomic output behavior, stable diagnostics, and locked builds remain intact.

## Task 1: Bound join stdin ingestion

### Scope

Replace the unbounded `read_to_end` path used when a join source is `-` with the same incremental bounded-reader strategy used for ordinary list input.

### Requirements

- Enforce both the per-source byte limit and the aggregate input-byte budget while reading.
- Stop reading as soon as the limit is exceeded; do not retain the excess buffer.
- Preserve `INPUT_TOO_LARGE`, `FILE_UNREADABLE`, and existing path context behavior.
- Apply the same behavior to JSONL, CSV, and TSV joins.
- Add a regression test that supplies more than the configured stdin limit and verifies a bounded failure.
- Add a test proving a valid stdin input at the limit remains accepted.

### Acceptance criteria

- No join input path can allocate an unbounded buffer from a hostile stream.
- A never-ending stdin stream is limited by bytes and can be terminated by the service wrapper.
- Existing join parsing tests continue to pass.

## Task 2: Redesign joins for bounded streaming

### Scope

Remove the requirement for the core join API to return a complete `Vec<JoinedRecord>` for normal execution. Keep the hash index where necessary, but stream output records through a callback or writer-oriented iterator.

### Requirements

- Preserve deterministic ordering and all four join types: inner, left, full, and anti.
- Preserve duplicate-key expansion and collision-renaming semantics.
- Enforce the maximum output-record budget before constructing or retaining the next result.
- Apply offset and limit during join traversal, not after a complete join result exists.
- Ensure `--limit 1` does not generate or retain unrelated output records.
- Ensure `--count-only` does not construct joined records or duplicate field strings.
- Keep output-byte enforcement authoritative during serialization.
- Define how full joins track unmatched right-side records without retaining joined output.
- Preserve stable errors such as `JOIN_LIMIT_EXCEEDED` and `OUTPUT_LIMIT_EXCEEDED`.

### Suggested API direction

Introduce a core operation resembling:

```text
join_each(left, right, keys, kind, limits, callback)
```

The callback should be invoked for each selected record and may stop traversal with a typed cancellation or output-limit error. A separate bounded count path should calculate counts without creating `JoinedRecord` values.

### Acceptance criteria

- Duplicate-key stress tests demonstrate bounded memory for a result near the record ceiling.
- A join with `--limit 1` emits one record without building the full result.
- Count-only tests demonstrate no joined-record materialization.
- Full, left, anti, unmatched-key, duplicate-key, and collision-renaming tests preserve current output.
- Failure before completion does not commit an output file.

## Task 3: Reduce input memory amplification

### Scope

Review the core parsers for avoidable whole-input copies and high-amplification intermediate structures.

### Requirements

- Replace escaped-inline `Vec<char>` processing with a bounded character iterator or equivalent single-pass parser.
- Avoid collecting all separator slices before converting them into owned strings.
- Keep item, aggregate-byte, list, and total-item limits enforced before retaining additional data.
- Preserve UTF-8 validation and escape semantics, including `\\xNN`, NUL, and delimiter handling.
- Measure peak memory for inputs near configured limits.

### Acceptance criteria

- Peak parser memory is documented as a bounded multiple of configured input and item limits.
- Hostile large inline, line, NUL, CSV, and TSV inputs terminate within expected resource bounds.
- Existing parser behavior and error codes remain unchanged.

## Task 4: Add cancellation and execution deadlines

### Scope

Expose cancellation through the reusable execution APIs and make the CLI/service integration able to terminate long-running requests.

### Requirements

- Retain the existing per-record cancellation hook in the core.
- Add a deadline-based helper or documented adapter that does not require callers to build their own timing logic.
- Check cancellation during expensive phases, not only before record emission: join indexing, normalization, parsing where practical, and large transformations.
- Ensure cancellation removes staged temporary output and never commits partial output.
- Preserve `CANCELLED` as the stable error code.
- Document that wall-clock, CPU, memory, and concurrency limits must also be enforced by the hosting service/process supervisor.

### Acceptance criteria

- A cancellation test stops generation promptly before the next record.
- A cancellation during join processing does not commit output.
- Service integration guidance includes a finite request deadline and process/resource isolation.

## Task 5: Harden hostile filesystem output handling

### Scope

Close or clearly constrain remaining path-resolution races around output directories, symlinks, and Windows reparse points.

### Requirements

- Review Unix and Windows commit operations separately.
- Prefer directory-handle-relative or equivalent no-follow operations where supported.
- Verify that the temporary file and destination remain in the intended directory before commit.
- Reject unsafe parent directories when the deployment threat model includes same-user attackers.
- Preserve atomic replacement and non-overwrite semantics.
- Add race-oriented tests where the platform permits, including destination replacement, dangling symlink, parent-directory replacement, and Windows reparse-point cases.
- Document the residual limitation if fully race-free behavior is not portable through the standard library.

### Acceptance criteria

- An attacker cannot cause overwrite mode to follow a destination symlink or reparse point.
- Non-overwrite mode cannot replace an existing destination, including one created after preflight.
- Failed runs remove only their own temporary file.
- Existing destination content remains intact on parse, cancellation, write, sync, and commit failure.

## Task 6: Add service-oriented resource policy guidance

### Scope

Document safe defaults and required controls for wrappers that expose the CLI or library over a network.

### Requirements

- Recommend substantially lower per-request limits than the 1 GiB CLI hard output ceiling.
- Require authentication/authorization where filesystem paths or output destinations are exposed.
- Recommend rejecting arbitrary filesystem paths or restricting access to an application-owned directory.
- Specify per-client and global concurrency limits.
- Specify wall-clock, CPU, memory, input-rate, output-rate, and disk quotas.
- Recommend running requests under a dedicated low-privilege account or sandbox/container.
- State that `--no-preflight` skips estimation only and does not provide capacity reservation.
- Document that preflight is advisory and must not replace runtime quotas.

### Acceptance criteria

- README or deployment documentation contains a public-service threat model and a concrete baseline policy.
- The documented policy distinguishes CLI defaults, hard ceilings, and service-wrapper limits.

## Task 7: Regression, fuzzing, and verification suite

### Tests to add

- Bounded join stdin with oversized and endless-reader behavior.
- Join duplicate-key expansion near the configured record limit.
- Join offset/limit and count-only memory behavior.
- Output-byte limit during streamed join serialization.
- Cancellation during product, join, normalization, and output.
- Large escaped inline input and parser peak-memory checks.
- Output path races and cleanup identity checks.
- Hostile JSONL depth, malformed records, duplicate fields, and oversized fields.

### Verification commands

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo audit --locked
```

Where practical, add fuzz targets for bounded parsers, template parsing, join records, and output serialization. Fuzz runs must have explicit time and memory limits.

## Recommended implementation order

1. Bound join stdin ingestion.
2. Redesign joins for streaming and bounded count-only operation.
3. Add join-specific output-byte enforcement and cancellation checks.
4. Reduce parser memory amplification.
5. Harden filesystem path handling and add race tests.
6. Add service policy documentation and integration examples.
7. Run full regression, fuzz, static, dependency, and clean-build verification.

Each implementation task should be developed on a dedicated branch, preserve unrelated user changes, add regression tests with the fix, and be reviewed for stable error behavior and resource accounting before merge.
