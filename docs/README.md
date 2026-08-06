# Documentation guide

This page routes readers to the shortest document that answers their question.
The generated CLI help remains the authority for the exact options supported
by an installed binary.

## I want to use the application

1. Read the [README quick start](../README.md#quick-start).
2. Choose the CLI, GUI, or TUI.
3. For CLI work, continue with the [CLI user manual](cli-usage.md).
4. Look up unfamiliar terms in the [glossary](glossary.md).

## I am writing automation

- Use the [CLI user manual](cli-usage.md) for input, output, templates,
  paging, and machine-readable plans.
- Use the [error reference](error-reference.md) for exit statuses and
  diagnostics.
- Read the [compatibility policy](compatibility.md) before depending on output
  ordering or JSON fields.
- Read [security and deployment](security-and-deployment.md) before handling
  untrusted input or exposing the tool through a service.

## I want to embed the Rust libraries

Read [library usage](library-usage.md) and the [Rust library API status in the
compatibility policy](compatibility.md#rust-library-api-status). The CLI is the
stable integration boundary; the Rust APIs are intentionally not semver-stable
before a concrete external consumer and supported API surface are identified.

## I maintain or release the project

- [Release procedure](release.md) — one cross-platform driver and one GitHub
  workflow
- [Active implementation plans](plans/README.md)
- [Dependency licenses](dependency-licenses.md)
- [Benchmarking guide](benchmarking.md)
- [Fuzzing guide](../fuzz/README.md)

## I need historical context

The [documentation archive](archive/README.md) contains earlier feature
roadmaps, brainstorming, design specifications, implementation plans,
security-remediation notes, and release-planning records. These files are
retained for project history and engineering research, but are not current
user guidance.
