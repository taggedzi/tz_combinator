# Security Remediation Plan

This project is intended to run in hostile or careless environments. The plan below addresses the security findings from the repository review in descending order of impact. Each implementation item is intended to be developed on its own `security/*` branch, committed, verified, and merged into `master` before the next item begins.

## Branch and merge policy

For every item:

1. Start from the current `master`.
2. Create a dedicated `security/<short-name>` branch.
3. Implement the fix and regression tests.
4. Run focused tests and relevant workspace checks.
5. Commit with a security-focused message.
6. Merge the branch into `master` with a merge commit.
7. Verify the merged result before creating the next branch.

## 1. Secure output creation and atomic replacement

Risk: a check-then-create race or symlink/reparse-point path can cause an unintended file to be overwritten. Direct writes can also destroy the previous output before generation succeeds.

Implementation:

- Replace the separate existence check and `File::create` operation with safe open semantics.
- Reject symlinks and Windows reparse points where supported.
- Use exclusive creation when overwrite is disabled.
- Write to a temporary file in the destination directory.
- Flush and sync successful output, then atomically replace the destination.
- Preserve the old destination if generation or writing fails.
- Clean up temporary files on all failure paths.

Acceptance tests:

- Existing files are preserved without overwrite permission.
- Symlinks and dangling symlinks are rejected safely.
- Failed generation does not replace the existing destination.
- Successful overwrite is atomic.
- Concurrent path changes cannot redirect output to another file.

## 2. Enforce output limits while streaming

Risk: preflight checks are not reservations and can be bypassed or invalidated. Unbounded stdout or file output can exhaust disk space or consume excessive time.

Implementation:

- Add a counting writer that enforces the configured byte limit before each record.
- Keep runtime enforcement active even with `--no-preflight`.
- Add a stable `OUTPUT_LIMIT_EXCEEDED` error.
- Decide and document a safe default for unbounded output in hostile deployments.
- Ensure failed output removes the temporary file and does not replace the destination.

Acceptance tests:

- Output never exceeds the configured limit.
- Limits are enforced at record boundaries.
- JSON escaping cannot bypass the limit.
- `--no-preflight` disables estimation only, not runtime enforcement.

## 3. Bound input and combination resources

Risk: file and stdin input are loaded completely into memory, and attacker-controlled list sizes can cause memory, CPU, or output denial of service.

Implementation:

- Replace unbounded `read_to_string` calls with incremental bounded readers.
- Add limits for total input bytes, lists, items per list, total items, and item length.
- Apply the same limits to files, stdin, and inline values.
- Add a safe maximum combination/output budget when no explicit limit is supplied.
- Add stable errors for each violated resource limit.
- Preserve checked arithmetic and fail before generation when a request is unsafe.

Acceptance tests:

- Inputs at the limit are accepted and inputs above it are rejected.
- Huge files and stdin streams terminate without unbounded memory growth.
- Too many lists/items and oversized items produce coded errors.
- Combination overflow and resource limits cannot cause a panic.

## 4. Make JSONL size estimation exact or conservative

Risk: the current estimator ignores JSON escaping, so preflight can underestimate output size for quotes, backslashes, control characters, and similar values.

Implementation:

- Share serialization-length logic with JSONL output formatting.
- Account for escaping in both `value` and `fields`.
- Calculate index width for the actual emitted index range.
- Select maximum records by serialized length rather than raw string length.
- Ensure checked arithmetic continues to report overflow.

Acceptance tests:

- Estimates never fall below actual output for lean and full JSONL.
- Tests cover quotes, backslashes, newlines, tabs, control characters, and Unicode.
- Offset, limit, empty-list, and overflow cases remain correct.

## 5. Fail closed when capacity cannot be determined

Risk: disk-space lookup currently falls back to `u64::MAX`, allowing preflight to approve output when capacity is unknown.

Implementation:

- Return a coded error when free-space lookup fails.
- Keep runtime byte limits mandatory even when preflight is bypassed.
- Document that free-space checks are advisory and do not reserve disk space.

Acceptance tests:

- Capacity lookup failure fails closed.
- `--no-preflight` still enforces output limits.
- Normal capacity checks continue to work.

## 6. Escape human-readable diagnostics

Risk: attacker-controlled paths can inject newlines or terminal control sequences into text stderr and logs.

Implementation:

- Escape control characters in text diagnostics.
- Keep JSON diagnostics structured and valid.
- Ensure text diagnostics remain one line.

Acceptance tests:

- Newlines, carriage returns, tabs, ANSI sequences, quotes, and Unicode are safely rendered.

## 7. Improve dependency and build reproducibility

Risk: the lockfile is ignored, so clean builds can resolve changing dependency versions.

Implementation:

- Track `Cargo.lock`.
- Require locked builds and tests.
- Pin the Rust toolchain for release and CI builds.
- Add dependency auditing and intentional update review.
- Verify release builds from a clean checkout.

Acceptance tests:

- A clean checkout builds with `--locked`.
- CI runs tests, formatting, clippy, and dependency auditing.

## 8. Documentation and final security verification

Update the README with resource limits, atomic output behavior, symlink handling, runtime versus preflight enforcement, new errors, and deployment guidance. Finish with a full regression suite covering hostile input, output races, JSON escaping, resource exhaustion, diagnostic injection, and reproducible builds.

## Implementation history

The plan was implemented sequentially, with each item developed and merged from a dedicated branch:

| Area | Branch | Implementation commit |
|---|---|---|
| Plan and agent guidance | `security/00-remediation-plan` | `43c5e43` |
| Secure output creation and replacement | `security/01-safe-output-files` | `376702f` |
| Runtime output limits | `security/02-enforce-output-limits` | `6878e85` |
| Bounded input and product resources | `security/03-bound-input-resources` | `4874f54` |
| Conservative JSONL estimates | `security/04-conservative-json-estimates` | `a9f2f6b` |
| Fail-closed capacity checks | `security/05-fail-closed-capacity` | `b4c879e` |
| Escaped diagnostics | `security/06-escape-diagnostics` | `6fa6428` |
| Locked dependency verification | `security/07-reproducible-builds` | `ca9c021` |
| Security documentation | `security/08-security-documentation` | `a7c871c` |
| Final history bookkeeping | `security/09-final-history` | `a56d773` |
| Documentation line-ending cleanup | `security/10-normalize-plan` | `f916635` |
| Windows reparse-point protection | `security/11-reparse-point-protection` | `8019a7d` |

All implementation branches were merged into local `master` with merge commits before the next branch was created.
