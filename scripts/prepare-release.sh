#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <version> [YYYY-MM-DD]" >&2
}

die() {
    echo "error: $*" >&2
    exit 1
}

if [[ $# -lt 1 || $# -gt 2 ]]; then
    usage
    exit 2
fi

version="$1"
release_date="${2:-$(date -u +%F)}"

[[ "$version" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] ||
    die "version must be a stable semantic version such as 0.2.0"
[[ "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] ||
    die "release date must use YYYY-MM-DD"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" ||
    die "run this command from a Git worktree"
cd "$repo_root"

[[ -z "$(git status --porcelain)" ]] ||
    die "the worktree must be clean before preparing a release"
git rev-parse --verify --quiet "refs/tags/v$version" >/dev/null &&
    die "tag v$version already exists"
[[ -f CHANGELOG.md && ! -L CHANGELOG.md ]] ||
    die "CHANGELOG.md must be a regular file"
[[ -f Cargo.lock && ! -L Cargo.lock ]] ||
    die "Cargo.lock must be a regular file"

manifests=(
    crates/combinator-app/Cargo.toml
    crates/combinator-cli/Cargo.toml
    crates/combinator-codecs/Cargo.toml
    crates/combinator-core/Cargo.toml
    crates/combinator-gui/Cargo.toml
    crates/combinator-tui/Cargo.toml
)
for manifest in "${manifests[@]}"; do
    [[ -f "$manifest" && ! -L "$manifest" ]] || die "$manifest must be a regular file"
done
benchmark_manifest="crates/combinator-benchmarks/Cargo.toml"
[[ -f "$benchmark_manifest" && ! -L "$benchmark_manifest" ]] ||
    die "$benchmark_manifest must be a regular file"
benchmark_package_matches="$(grep -c '^version = "0.0.0"$' "$benchmark_manifest")"
[[ "$benchmark_package_matches" == 1 ]] ||
    die "$benchmark_manifest package version must remain 0.0.0"

old_version=""
for manifest in "${manifests[@]}"; do
    manifest_version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$manifest")"
    [[ "$manifest_version" =~ ^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] ||
        die "$manifest does not contain exactly one stable package version"
    if [[ -z "$old_version" ]]; then
        old_version="$manifest_version"
    elif [[ "$manifest_version" != "$old_version" ]]; then
        die "workspace package versions are not synchronized"
    fi
done

IFS=. read -r old_major old_minor old_patch <<< "$old_version"
IFS=. read -r new_major new_minor new_patch <<< "$version"
if (( new_major < old_major ||
      (new_major == old_major && new_minor < old_minor) ||
      (new_major == old_major && new_minor == old_minor && new_patch <= old_patch) )); then
    die "new version $version must be greater than current version $old_version"
fi

unreleased_body="$(
    awk '
        /^## \[Unreleased\]$/ { found = 1; in_section = 1; next }
        in_section && /^## \[/ { in_section = 0 }
        in_section && NF { print }
        END { if (!found) exit 2 }
    ' CHANGELOG.md
)" || die "CHANGELOG.md is missing an Unreleased heading"
[[ -z "$unreleased_body" ]] ||
    die "the Unreleased section must be empty before deterministic generation"

notes_file="release-notes/$version.md"
[[ ! -e "$notes_file" && ! -L "$notes_file" ]] ||
    die "$notes_file already exists"
[[ -d release-notes && ! -L release-notes ]] ||
    die "release-notes must be an existing, non-symlink directory"
scripts/render-release-notes.sh "$version" "$release_date" "$notes_file"
scripts/sync-release-notes.sh "$version"

update_manifest() {
    local manifest="$1"
    local tmp_manifest
    tmp_manifest="$(mktemp "$(dirname "$manifest")/.Cargo.toml.XXXXXX")"
    awk -v old="$old_version" -v new="$version" '
        $0 == "version = \"" old "\"" {
            print "version = \"" new "\""
            next
        }
        /^combinator-[a-z-]+ = \{ version = "/ &&
            index($0, "version = \"" old "\"") {
            sub("version = \"" old "\"", "version = \"" new "\"")
        }
        { print }
    ' "$manifest" > "$tmp_manifest"
    chmod --reference="$manifest" "$tmp_manifest"
    mv "$tmp_manifest" "$manifest"
}

update_internal_dependencies() {
    local manifest="$1"
    local tmp_manifest
    tmp_manifest="$(mktemp "$(dirname "$manifest")/.Cargo.toml.XXXXXX")"
    awk -v old="$old_version" -v new="$version" '
        /^combinator-[a-z-]+ = \{ version = "/ &&
            index($0, "version = \"" old "\"") {
            sub("version = \"" old "\"", "version = \"" new "\"")
        }
        { print }
    ' "$manifest" > "$tmp_manifest"
    chmod --reference="$manifest" "$tmp_manifest"
    mv "$tmp_manifest" "$manifest"
}

for manifest in "${manifests[@]}"; do
    update_manifest "$manifest"
done
update_internal_dependencies "$benchmark_manifest"

lock_tmp="$(mktemp ./.Cargo.lock.XXXXXX)"
awk -v old="$old_version" -v new="$version" '
    /^\[\[package\]\]$/ { workspace_package = 0 }
    /^name = "combinator-(app|cli|codecs|core|gui|tui)"$/ {
        workspace_package = 1
    }
    workspace_package && $0 == "version = \"" old "\"" {
        print "version = \"" new "\""
        next
    }
    { print }
' Cargo.lock > "$lock_tmp"
chmod --reference=Cargo.lock "$lock_tmp"
mv "$lock_tmp" Cargo.lock

scripts/verify-release.sh "$version"

echo "Prepared release v$version ($release_date). Review the generated changes before committing."
