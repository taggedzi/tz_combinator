# Contributing to tz_combinator

Thank you for considering a contribution. Bug reports, documentation fixes,
tests, design feedback, and code changes are all valuable.

This is an unpaid, single-maintainer project with limited capacity. There is
no guaranteed response or review time. Contributions may be deferred or
declined because of scope, maintenance cost, risk, or available energy. That
is not a judgment about the contributor or the quality of the idea.

## A low-friction way to help

- For a bug, use the
  [bug report form](https://github.com/taggedzi/tz_combinator/issues/new?template=bug_report.yml).
- For an improvement, use the
  [feature request form](https://github.com/taggedzi/tz_combinator/issues/new?template=feature_request.yml).
- For documentation, use the
  [documentation form](https://github.com/taggedzi/tz_combinator/issues/new?template=documentation.yml).
- If none fits, opening a blank issue is fine.
- Small, clear fixes may be submitted directly as a pull request.

Please do not include secrets, private data, or sensitive vulnerability
details in a public issue. Follow [SECURITY.md](SECURITY.md) for security
reports.

You do not need to disclose a disability or personal circumstance. If a
template or process is inaccessible, use a blank issue or contribute the same
information in a format that works for you.

## Before starting a change

A prior issue is helpful, but not required, for small bug fixes,
documentation, and tests. Please discuss a change first when it:

- changes CLI behavior, output ordering, error codes, or compatibility;
- affects filesystem or resource-limit security;
- adds a dependency, network access, unsafe Rust, or a new interface;
- requires a substantial redesign or long-term maintenance commitment; or
- is likely to span several crates.

Early discussion can prevent work on a direction the project cannot maintain.
It does not guarantee that a proposal will be accepted.

## Development setup

The workspace uses Rust 2021 and Rust 1.94.1. Build with the committed lockfile:

```text
cargo build --workspace --locked
```

The main crates are described in [README.md](README.md#project-status-and-compatibility).
CLI behavior is documented in [docs/cli-usage.md](docs/cli-usage.md), and the
public compatibility contract is in
[docs/compatibility.md](docs/compatibility.md).

## Making a change

1. Fork or branch from the current `master`.
2. Keep the change focused. Avoid unrelated formatting or refactoring.
3. Add or update tests when behavior changes.
4. Update user documentation when the public interface changes.
5. Run the smallest relevant checks, then broader checks when practical.
6. Open a pull request explaining the problem, approach, and verification.

Clear, ordinary commit messages are sufficient; a signed commit or contributor
agreement is not required. When practical, use a Conventional Commit prefix
for a user-visible change so automated release preparation can categorize it:

```text
feat: add a new user-visible capability
fix: correct user-visible behavior
security: harden a user-visible security boundary
```

Use `!` for a breaking change, such as `feat!: change a stable CLI contract`.
Documentation, test, CI, refactoring, style, build, and chore commits are
excluded from generated user-facing notes. Maintainers review the generated
notes, so an imperfect or unconventional contributor commit message does not
block a contribution.

## Project standards

`tz_combinator` is intended for hostile or careless environments. Changes
must preserve predictable resource use and fail safely.

- Treat inputs, paths, environment values, and filesystem state as untrusted.
- Bound bytes, item counts, output, combinations, recursion, and other work.
- Use checked arithmetic for counts and sizes.
- Preserve streaming behavior where buffering could become unbounded.
- Treat preflight checks as advisory unless enforced at the point of use.
- Consider symlinks, reparse points, races, partial writes, and atomic
  replacement for filesystem changes.
- Do not weaken a security check merely to make a test pass.
- Preserve stable CLI behavior unless an approved change explicitly updates
  the compatibility contract.

New shell execution, network access, dynamic loading, dependencies, or unsafe
Rust require explicit justification and careful review.

## Verification

For Rust changes, run the applicable commands:

```text
cargo test -p <affected-package> --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

It is okay to submit a useful contribution when you cannot run every check.
State what you did and did not run. Continuous integration will run the
project checks, and the maintainer may ask for targeted follow-up.

Documentation-only changes generally need link, formatting, and example
checks rather than the complete Rust suite.

## Pull request review

Reviews prioritize correctness, security, compatibility, clarity, and future
maintenance cost. A review may ask for a smaller scope, additional tests, or a
simpler design.

To conserve maintainer and contributor time:

- drafts and incomplete work are welcome when clearly marked;
- small pull requests are usually easier to review;
- unresolved review questions may pause a pull request;
- inactive pull requests may be closed without prejudice and reopened later;
  and
- maintainers may finish, adapt, or supersede an idea with appropriate credit.

Please disclose material limitations in verification. If automated or AI tools
substantially produced a change, review their output carefully and mention the
assistance when it would help reviewers understand provenance or risk. The
contributor remains responsible for the submitted content.

## Licensing and conduct

By submitting a contribution, you agree that it may be distributed under the
project's [MIT License](LICENSE).

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).
