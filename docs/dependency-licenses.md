# Dependency licensing

`tz_combinator` and its workspace crates are licensed under MIT. Third-party
dependencies remain under their own licenses; adding a dependency does not
change the license of this project.

## CSV support

The CLI uses the `csv` crate for CSV and TSV parsing. The locked dependency
versions currently include:

| Crate | Version | License | Use |
|---|---:|---|---|
| `csv` | 1.4.0 | MIT or Unlicense | Quoted CSV/TSV reader |
| `csv-core` | 0.1.13 | MIT or Unlicense | CSV parser core |
| `itoa` | 1.0.18 | MIT or Apache-2.0 | Transitive formatting dependency |
| `ryu` | 1.0.23 | Apache-2.0 or BSL-1.0 | Transitive formatting dependency |
| `serde_core` | 1.0.229 | MIT or Apache-2.0 | Transitive data dependency |
| `memchr` | 2.8.3 | MIT or Unlicense | Transitive parser dependency |

The `csv` license is documented by [docs.rs](https://docs.rs/crate/csv/1.4.0),
and its source is maintained at
[BurntSushi/rust-csv](https://github.com/BurntSushi/rust-csv). The project
distributes the dependency notices required by the selected licenses; legal
review should confirm whether the Unlicense option is acceptable for a given
distribution policy.

## Benchmark development tooling

The non-published `combinator-benchmarks` package keeps its tools outside the
production dependency graph. The exact locked direct/report dependencies are:

| Crate | Version | License | Use |
|---|---:|---|---|
| `criterion` | 0.8.2 | MIT or Apache-2.0 | Statistical benchmark harness and optional HTML reports |
| `criterion-plot` | 0.8.2 | MIT or Apache-2.0 | Criterion plot-data support |
| `plotters` | 0.3.7 | MIT | Opt-in SVG charts for readable reports |
| `plotters-backend` | 0.3.7 | MIT | Plotters rendering interface |
| `plotters-svg` | 0.3.7 | MIT | SVG report rendering |
| `tempfile` | 3.27.0 | MIT or Apache-2.0 | Dedicated directories for file benchmarks |

Criterion and Tempfile are used under their MIT option. Plotters is enabled
only by the `benchmark-report` feature. These dependencies do not change the
MIT license of the workspace or any first-party source. `deny.toml` includes
development dependencies, and CI checks all features so future benchmark and
report dependency changes fail closed against the approved license/source
policy. Non-Cargo workflow actions and report assets still require manual
review. See the [benchmarking guide](benchmarking.md) for the selection record
and opt-in report commands.

## Audit procedure

Before releasing a binary or changing dependencies:

1. Review `Cargo.lock` for the exact resolved dependency set.
2. Run `cargo deny --all-features check` and a license inventory tool such as
   `cargo license` or `cargo-about`.
3. Review every normal and platform-specific dependency, including
   transitive dependencies.
4. Preserve the generated third-party notices with the release artifacts.

The lockfile is committed so dependency versions and checksums are
reproducible. A dependency with a license that is not approved by the project
policy must not be added without an explicit licensing decision.

CI enforces the approved license/source policy from `deny.toml` with
`cargo deny check` and runs `cargo audit` against the committed
lockfile. Release archives retain `THIRD_PARTY_LICENSES.md` alongside the
project license. At the time of this release review, `cargo audit` reports two
allowed upstream maintenance warnings with no safe upgrade available:
`paste` through the platform-specific Metal renderer and `ttf-parser` through
the iced font stack. They are explicitly recorded in `deny.toml` and must be
re-reviewed whenever the GUI dependency graph changes.
