#!/usr/bin/env bash
set -euo pipefail

die() {
    echo "error: $*" >&2
    exit 1
}

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <version>" >&2
    exit 2
fi

version="$1"
[[ "$version" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] ||
    die "version must be a stable semantic version such as 0.2.0"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" ||
    die "run this command from a Git worktree"
cd "$repo_root"

notes_file="release-notes/$version.md"
[[ -f "$notes_file" && ! -L "$notes_file" ]] ||
    die "$notes_file must be a regular file"
[[ -f CHANGELOG.md && ! -L CHANGELOG.md ]] ||
    die "CHANGELOG.md must be a regular file"

version_pattern="${version//./\\.}"
heading_pattern="^## \\[$version_pattern\\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$"
heading_count="$(grep -Ec "$heading_pattern" "$notes_file")"
[[ "$heading_count" == 1 ]] ||
    die "$notes_file must contain exactly one dated heading for $version"
release_date="$(grep -E "$heading_pattern" "$notes_file" | sed 's/^.* - //')"
date -u -d "$release_date" +%F 2>/dev/null | grep -Fxq "$release_date" ||
    die "$notes_file contains an invalid release date"
grep -q '^- ' "$notes_file" ||
    die "$notes_file must contain at least one changelog entry"
other_headings="$(grep -Ec '^## \[' "$notes_file")"
[[ "$other_headings" == 1 ]] ||
    die "$notes_file must contain only one release section"

existing_heading_count="$(grep -Ec "$heading_pattern" CHANGELOG.md || true)"
if [[ "$existing_heading_count" == 0 ]]; then
    start_line="$(grep -n -m1 '^## \[[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\]' CHANGELOG.md | cut -d: -f1)"
    [[ -n "$start_line" ]] || die "CHANGELOG.md has no existing release section"
    suffix_line="$start_line"
else
    [[ "$existing_heading_count" == 1 ]] ||
        die "CHANGELOG.md contains duplicate sections for $version"
    start_line="$(grep -n -m1 -E "$heading_pattern" CHANGELOG.md | cut -d: -f1)"
    next_heading_offset="$(
        tail -n "+$((start_line + 1))" CHANGELOG.md |
            grep -n -m1 '^## \[' |
            cut -d: -f1 || true
    )"
    if [[ -n "$next_heading_offset" ]]; then
        suffix_line=$((start_line + next_heading_offset))
    else
        suffix_line=$(( $(wc -l < CHANGELOG.md) + 1 ))
    fi
fi

new_changelog="$(mktemp ./.CHANGELOG.md.XXXXXX)"
trap 'rm -f "$new_changelog"' EXIT
{
    sed -n "1,$((start_line - 1))p" CHANGELOG.md
    cat "$notes_file"
    echo
    if (( suffix_line <= $(wc -l < CHANGELOG.md) )); then
        sed -n "${suffix_line},\$p" CHANGELOG.md
    fi
} > "$new_changelog"
chmod --reference=CHANGELOG.md "$new_changelog"
mv "$new_changelog" CHANGELOG.md
