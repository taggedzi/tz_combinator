# Glossary

This glossary defines the combinatorics, data, and safety terms used by
`tz_combinator`.

## Operations

### Cartesian product

Every possible record formed by choosing one item from each input list. For
lists `[red, blue]` and `[car, bike]`, the Cartesian product contains
`red-car`, `red-bike`, `blue-car`, and `blue-bike`. The CLI operation is
`product`, and it is the default when no operation is named.

### Zip

Positional pairing across lists: the first items form one record, the second
items form the next, and so on. `--on-unequal` controls what happens when the
lists have different lengths.

### Concatenation

Sequentially emitting all items from the first list, then the second list, and
so on. The CLI operation is `concat`. Each output record contains one field.

### Permutation

An ordering of every item in one input pool. Order matters: `abc` and `bac`
are different permutations.

### Combination

An unordered selection of a fixed size from one input pool. For a selection
size of two, `ab` and `ba` describe the same combination and only one is
emitted.

### Variation

An ordered selection of a fixed size without replacement. Unlike a
permutation, a variation does not have to use every item. Unlike a
combination, order matters.

### Keyed join

An operation that matches records from two structured inputs by the values in
named key fields. A join can keep only matching records (`inner`), preserve
unmatched left records (`left`), preserve unmatched records from both sides
(`full`), or emit only unmatched left records (`anti`).

## Inputs and output

### Field

One value within a generated record. In a product, each input list contributes
one field. Fields may be referenced by zero-based position, such as `{0}`, or
by a name supplied with `--name`.

### Record

One generated output unit. In text output it ends with the record separator.
In JSON Lines output it occupies one line.

### Standard output and standard error

The two process streams used by the CLI. Standard output (`stdout`) contains
generated data. Standard error (`stderr`) contains diagnostics, warnings, and
optional summaries. Automation should read the streams separately.

### Input pool

The single input list used by permutations, combinations, and variations.
These operations reject requests containing more than one input list.

### Template

A bounded output pattern containing literal text and field placeholders, such
as `https://{host}:{port}`. Templates do not execute code, run commands, read
environment variables, or access files.

### Transform

A normalization step applied independently to each input list before counting
and generation. Examples include trimming whitespace, sorting, filtering, or
removing duplicates.

### Filter

A side-effect-free predicate applied to a candidate record. Repeated filters
are combined with logical AND, so every filter must match.

### JSON Lines (JSONL)

A streaming format containing one complete JSON value per line. Full CLI
output uses an object with an index, rendered value, and fields. Lean output
uses one JSON string per line.

### NUL-delimited

A byte-oriented format in which a zero byte (`\0`) separates records. It is
useful when values may contain newlines.

## Ordering and work distribution

### Rightmost-fastest order

The default product order. The item selected from the rightmost input list
changes on each record, like the last digit of an odometer.

### Offset and limit

Paging controls. The offset skips a number of records in the selected order;
the limit caps how many following records are emitted.

### Shard

One balanced, contiguous section of the ordered output. A zero-based shard
index selects a section, and the shard count specifies the total number of
sections.

### Half-open range

A range that includes its start position and excludes its end position. For
example, `[3, 5)` contains positions 3 and 4. Shard boundaries use this form
so adjacent shards do not overlap.

## Safety and compatibility

### Preflight

Validation and size estimation performed before generation. A preflight disk
check does not reserve space and can become stale, so runtime limits remain
authoritative.

### Compiled ceiling

A hard security maximum built into the program. A command-line option may
lower the effective limit but cannot raise it above this ceiling.

### Atomic replacement

Writing replacement content to a sibling temporary file and committing it
only after the write succeeds. This prevents a failed write from leaving a
partially replaced destination.

### TOCTOU

“Time of check to time of use”: a race in which filesystem state changes
after validation but before an operation uses it. Preflight checks are
advisory unless the condition is enforced again at the point of use.

### Symlink and reparse point

Filesystem objects that redirect access to another location. Reparse points
are the broader Windows mechanism that includes symbolic links. Output and
profile paths reject these redirections to prevent writes from escaping the
intended location.

### Stable input

Input whose values and order do not change between runs. Deterministic
ordering and sharding guarantees assume stable input.

### Semantic versioning (semver)

A versioning convention using `major.minor.patch`. This project treats the CLI
as its stable integration boundary; the Rust library APIs are not yet promised
to remain compatible across releases.
