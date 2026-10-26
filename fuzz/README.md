# Bounded fuzz targets

The targets operate only on in-memory data and cap input-derived work at
4 KiB/64 records. Fuzzing requires nightly Rust for sanitizer instrumentation.
On Linux, install the same pinned toolchains and `cargo-fuzz` version as CI.
Build `cargo-fuzz` with stable Rust: its locked `rustix` dependency uses compiler
internals that are incompatible with the pinned nightly. Nightly is used only
to build and run the instrumented fuzz targets.

```text
rustup toolchain install 1.94.1 --profile minimal
rustup toolchain install nightly-2026-10-01 --profile minimal
cargo +1.94.1 install cargo-fuzz --version 0.12.0 --locked
```

From the repository root, run a short local smoke campaign. `cargo-fuzz`
automatically finds the `fuzz/` project. The 512 MiB process budget includes
AddressSanitizer overhead: its default quarantine can retain 256 MiB of freed
allocations. The targets' input and output bounds remain unchanged.

```text
cargo +nightly-2026-10-01 fuzz run inline -- -max_total_time=15 -rss_limit_mb=512
cargo +nightly-2026-10-01 fuzz run template -- -max_total_time=15 -rss_limit_mb=512
cargo +nightly-2026-10-01 fuzz run join_jsonl -- -max_total_time=15 -rss_limit_mb=512
cargo +nightly-2026-10-01 fuzz run output -- -max_total_time=15 -rss_limit_mb=512
```

CI runs this bounded smoke campaign on relevant pull requests, on a schedule,
and on manual dispatch.
