#!/usr/bin/env bash
set -euo pipefail

readonly GIT_CLIFF_VERSION="2.12.0"

usage() {
    echo "Usage: $0 <version> <YYYY-MM-DD> <output-file>" >&2
}

die() {
    echo "error: $*" >&2
    exit 1
}

if [[ $# -ne 3 ]]; then
    usage
    exit 2
fi

version="$1"
release_date="$2"
output_file="$3"

[[ "$version" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] ||
    die "version must be a stable semantic version such as 0.2.0"
[[ "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] ||
    die "release date must use YYYY-MM-DD"
date -u -d "$release_date" +%F 2>/dev/null | grep -Fxq "$release_date" ||
    die "release date is not a valid calendar date"
[[ "$output_file" != "CHANGELOG.md" ]] ||
    die "refusing to overwrite CHANGELOG.md directly"
[[ ! -L "$output_file" ]] || die "output file must not be a symlink"
output_dir="$(dirname "$output_file")"
[[ -d "$output_dir" && ! -L "$output_dir" ]] ||
    die "output parent must be an existing, non-symlink directory"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" ||
    die "run this command from a Git worktree"
cd "$repo_root"

command -v git-cliff >/dev/null 2>&1 ||
    die "git-cliff $GIT_CLIFF_VERSION is required"
actual_version="$(git-cliff --version)"
[[ "$actual_version" == "git-cliff $GIT_CLIFF_VERSION" ]] ||
    die "expected git-cliff $GIT_CLIFF_VERSION, found: $actual_version"

tmp_output="$(mktemp "${TMPDIR:-/tmp}/tz-combinator-notes.XXXXXX")"
final_output="$(mktemp "$output_dir/.release-notes.XXXXXX")"
trap 'rm -f "$tmp_output" "$final_output"' EXIT

if git rev-parse --verify --quiet "refs/tags/v$version^{commit}" >/dev/null; then
    tag_commit="$(git rev-parse "refs/tags/v$version^{commit}")"
    head_commit="$(git rev-parse HEAD)"
    [[ "$tag_commit" == "$head_commit" ]] ||
        die "tag v$version does not point to HEAD"
    range_args=(--current)
else
    range_args=(--unreleased --tag "$version")
fi

git-cliff \
    --config cliff.toml \
    --no-exec \
    "${range_args[@]}" \
    --output "$tmp_output"

# An empty git-cliff header/footer still renders surrounding newlines. Remove
# only those boundary lines; whitespace inside the release section is stable.
sed -i '/./,$!d' "$tmp_output"
sed -i ':a;/^[[:space:]]*$/{$d;N;ba}' "$tmp_output"

expected_heading="## [$version]"
actual_heading="$(sed -n '1p' "$tmp_output")"
[[ "$actual_heading" == "$expected_heading" ]] ||
    die "git-cliff produced an unexpected heading: $actual_heading"
grep -q '^- ' "$tmp_output" ||
    die "no user-visible feat, fix, perf, revert, security, or breaking commits were found"

sed "1s/^## \\[$version\\]$/## [$version] - $release_date/" "$tmp_output" > "$final_output"
mv "$final_output" "$output_file"
