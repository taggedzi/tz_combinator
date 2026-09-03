# Changelog

This file records user-visible changes. Release sections are generated during
release preparation and synchronized from the reviewed sources in
`release-notes/`. Tagged releases also receive GitHub's automatically generated
release notes.

## [Unreleased]

## [0.3.0] - 2026-08-06

### Added

- Unify release process
- Confirm python in security.yml workflow
- Add downstream formula risk policy

### Fixed

- Guard raw terminal output
- Enforce shared safety ceilings
- Error/warning output flow control

## [0.2.2] - 2026-08-05

### Security

- Harden glob matching against denial of service.

## [0.2.1] - 2026-08-03

### Breaking Changes

- **Breaking:** Require bounded library rendering

### Fixed

- Remove unintended header
- Enforce resource limits before expansion

## [0.2.0] - 2026-08-03

### Security

- Recheck staged output destinations

### Added

- Automate release changelog preparation
- Add performance benchmark suite
- Add opt-in structured logging

### Fixed

- Launch GUI without a Windows console
- Exclude benchmark crate from version checks
- Mark release scripts executable
- Align benchmark dependency requirements

### Changed

- Avoid zero-page join residency
- Optimize core selection and duplicate joins

## [0.1.0] - 2026-07-26

- First early public release of the `combinator` CLI, TUI, GUI, and workspace
  libraries.
- Linux and Windows x86_64 release archives include all three binaries.
- The CLI is the supported integration boundary; library, GUI, and TUI APIs may
  change before `1.0.0`.
