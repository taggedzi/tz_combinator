# Pre-release Checklist Completion Plan

The repository now contains the CI and release workflows, dependency policy,
fuzz smoke job, release profile, packaging, checksum verification, and release
procedure described by this plan. The final remaining step is to execute the
workflow on the hosted Linux and Windows runners before publishing a tag.

This plan addresses the remaining pre-release checklist gaps identified in the
repository audit. macOS CI and release coverage is explicitly out of scope;
the supported release platforms remain Linux and Windows.

## 1. Define and document the compatibility policy

Update `README.md` with an explicit policy covering:

- CLI flags and defaults remain compatible within a major version.
- Existing error codes never change meaning; new failures receive new codes.
- Existing exit-code behavior and stdout/stderr discipline are preserved.
- JSONL fields retain their meaning; additive fields are permitted only when
  consumers can safely ignore them.
- `--explain --format json` changes require a new `schema_version` when they
  are not backward-compatible.
- `combinator-core` is not currently a supported semver-stable public Rust
  API; the CLI is the supported integration boundary.

Add a short reference to this policy in `CHANGELOG.md`, or create a dedicated
`docs/compatibility.md` if the policy becomes substantial.

### Acceptance criteria

- A user can determine whether CLI invocations, diagnostics, and JSON
  consumers remain compatible after an upgrade.
- The policy distinguishes current guarantees from future API plans.

## 2. Add golden CLI contract tests

Create a dedicated integration test module and fixture directory, for example:

```text
crates/combinator-cli/tests/contract.rs
crates/combinator-cli/tests/golden/
```

Add deterministic contract cases for:

- stdout records in text mode;
- stderr diagnostics and warnings;
- success, usage-error, and runtime-error exit codes;
- full JSONL record shape;
- lean JSONL shape; and
- the complete `--explain --format json` schema and representative values.

Use exact golden output where byte stability is intentional. Parse JSON for
semantic assertions, including required keys, value types, and
`schema_version`.

Cover product, zip, concat, and join where the format applies.

### Acceptance criteria

- Unexpected deterministic stdout/stderr changes fail the tests.
- Exit-code expectations are explicit for every contract case.
- JSON tests validate required fields and types rather than only selected
  values.

## 3. Add concurrent stdout/stderr subprocess tests

Add a reusable integration-test helper that:

1. Spawns the binary with both stdout and stderr piped.
2. Starts independent readers for both streams.
3. Waits for the process.
4. Joins both readers and returns the status and captured data.

Use it for cases that exercise both channels and potential pipe backpressure,
including:

- many records with `--summary`;
- warning-heavy input;
- JSONL diagnostics; and
- failure after partial output.

### Acceptance criteria

- These tests explicitly drain stdout and stderr concurrently.
- They do not rely solely on `.output()` or `.wait_with_output()`.
- They complete reliably without deadlock.

## 4. Add property tests

Add a bounded property-testing dependency such as `proptest`. Create focused
properties for:

- input parsing and escaped delimiters;
- template parsing and rendering;
- JSONL join parsing and malformed records;
- output formatting and JSON escaping;
- output-size estimates never underestimating actual output; and
- ordering and count consistency across operations.

Generated inputs must have explicit size and case limits.

### Acceptance criteria

- Arbitrary generated inputs do not cause panics.
- Valid generated records round-trip through supported codecs.
- Unicode, quotes, separators, and control characters remain correctly
  escaped and bounded.
- Resource-limit failures remain fail-closed.

## 5. Add fuzz targets and bounded CI smoke runs

Add a fuzzing setup under `fuzz/` with initial targets for:

- inline/list input parsing;
- template parsing;
- JSONL join parsing; and
- output formatting/serialization.

Fuzz targets must use bounded inputs and must not access the network, execute
processes, or depend on uncontrolled filesystem state.

Add a manual or scheduled CI workflow that runs each target for a short,
fixed duration. Keep long fuzz campaigns outside ordinary pull-request CI.

### Acceptance criteria

- Fuzz targets compile and have documented local commands.
- CI performs bounded fuzz smoke runs.
- Fuzzing cannot introduce unbounded allocation or execution behavior.

## Implementation order

1. Define the compatibility policy.
2. Add golden contract tests.
3. Add the concurrent subprocess helper and tests.
4. Add property tests.
5. Add fuzz targets and bounded CI execution.
6. Run the full verification set and update the release checklist.

## Final verification

Run:

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo package --workspace --locked --no-verify
```
