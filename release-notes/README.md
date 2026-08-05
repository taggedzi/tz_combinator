# Release-note fragments

The `prepare` operation of the Release workflow creates one Markdown source
file here for each release prepared by this system. The fragment begins as
deterministic `git-cliff` output from user-visible conventional commits since
the previous tag.

Review and edit the fragment rather than editing the corresponding section in
`CHANGELOG.md` directly. Then synchronize and verify it:

```text
python scripts/release.py sync 0.2.0
python scripts/release.py verify 0.2.0
```

Released fragments remain committed so the release workflow can prove that
the packaged changelog contains the reviewed text exactly.
