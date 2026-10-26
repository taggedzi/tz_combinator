# Bounded fuzz targets

The targets operate only on in-memory data and cap input-derived work at
4 KiB/64 records. Fuzzing requires nightly Rust for sanitizer instrumentation.
On Linux, install the same pinned toolchain and `cargo-fuzz` version as CI:

```text
rustup toolchain install nightly-2026-10-01 --profile minimal
cargo +nightly-2026-10-01 install cargo-fuzz --version 0.12.0 --locked
```

From the repository root, run a short local smoke campaign. `cargo-fuzz`
automatically finds the `fuzz/` project:

```text
cargo +nightly-2026-10-01 fuzz run inline -- -max_total_time=15 -rss_limit_mb=256
cargo +nightly-2026-10-01 fuzz run template -- -max_total_time=15 -rss_limit_mb=256
cargo +nightly-2026-10-01 fuzz run join_jsonl -- -max_total_time=15 -rss_limit_mb=256
cargo +nightly-2026-10-01 fuzz run output -- -max_total_time=15 -rss_limit_mb=256
```

Long campaigns are intentionally left to scheduled or manually dispatched CI.
