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

[[ -f CHANGELOG.md && ! -L CHANGELOG.md ]] ||
    die "CHANGELOG.md must be a regular file"
[[ -f Cargo.lock && ! -L Cargo.lock ]] ||
    die "Cargo.lock must be a regular file"
notes_file="release-notes/$version.md"
[[ -f "$notes_file" && ! -L "$notes_file" ]] ||
    die "$notes_file must be a regular file"

version_pattern="${version//./\\.}"
heading_pattern="^## \\[$version_pattern\\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$"
mapfile -t headings < <(grep -E "$heading_pattern" CHANGELOG.md)
[[ ${#headings[@]} -eq 1 ]] ||
    die "CHANGELOG.md must contain exactly one dated section for $version"
release_date="${headings[0]##* - }"
date -u -d "$release_date" +%F 2>/dev/null | grep -Fxq "$release_date" ||
    die "CHANGELOG.md contains an invalid release date for $version"

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
for manifest in "${manifests[@]}"; do
    package_matches="$(grep -c "^version = \"$version\"$" "$manifest")"
    [[ "$package_matches" == 1 ]] ||
        die "$manifest package version is not $version"
    while IFS= read -r dependency; do
        [[ "$dependency" == *"version = \"$version\""* ]] ||
            die "$manifest has a stale internal dependency: $dependency"
    done < <(grep -E '^combinator-[a-z-]+ = \{.*path = ' "$manifest" || true)
done
while IFS= read -r dependency; do
    [[ "$dependency" == *"version = \"$version\""* ]] ||
        die "$benchmark_manifest has a stale internal dependency: $dependency"
done < <(grep -E '^combinator-[a-z-]+ = \{.*path = ' "$benchmark_manifest" || true)

lock_matches="$(
    awk -v version="$version" '
        /^\[\[package\]\]$/ { workspace_package = 0 }
        /^name = "combinator-(app|cli|codecs|core|gui|tui)"$/ {
            workspace_package = 1
            packages += 1
        }
        workspace_package && $0 == "version = \"" version "\"" {
            matches += 1
        }
        END {
            if (packages != 6 || matches != 6) exit 1
            print matches
        }
    ' Cargo.lock
)" || die "Cargo.lock workspace versions are not synchronized at $version"
[[ "$lock_matches" == 6 ]] || die "unexpected workspace package count in Cargo.lock"

expected_notes="$(mktemp "${TMPDIR:-/tmp}/tz-combinator-expected.XXXXXX")"
actual_notes="$(mktemp "${TMPDIR:-/tmp}/tz-combinator-actual.XXXXXX")"
trap 'rm -f "$expected_notes" "$actual_notes"' EXIT

cp "$notes_file" "$expected_notes"
awk -v heading="## [$version] - $release_date" '
    $0 == heading { found = 1; in_section = 1 }
    in_section && $0 != heading && /^## \[/ { exit }
    in_section { print }
    END { if (!found) exit 2 }
' CHANGELOG.md > "$actual_notes"

# Normalize only trailing blank lines around the extracted section. The
# reviewed release-note fragment is the canonical input after generation.
sed -i ':a;/^[[:space:]]*$/{$d;N;ba}' "$expected_notes" "$actual_notes"
diff -u "$expected_notes" "$actual_notes" ||
    die "the committed changelog section does not match $notes_file"

echo "Release metadata for v$version is synchronized and reproducible."
