# Bounded fuzz targets

The targets operate only on in-memory data and cap input-derived work at
4 KiB/64 records. Install `cargo-fuzz`, then run a short local smoke campaign:

```text
cargo fuzz run inline -- -max_total_time=30
cargo fuzz run template -- -max_total_time=30
cargo fuzz run join_jsonl -- -max_total_time=30
cargo fuzz run output -- -max_total_time=30
```

Long campaigns are intentionally left to scheduled or manually dispatched CI.
