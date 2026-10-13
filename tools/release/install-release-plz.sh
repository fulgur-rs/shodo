#!/usr/bin/env bash
set -euo pipefail

# Keep the release tool at the version selected by the reviewed v0.5 action.
# Digest from the official release-plz-v0.3.172 GitHub release asset metadata.
# Updating either value requires reviewing and verifying the new archive.
release_plz_version=0.3.172
release_plz_sha256=db58a4668ce9ef0b6431e96a665715745e9a5d7f0b0b28a610034d94de3970df

: "${RUNNER_TEMP:?GitHub runner temporary directory required}"
: "${GITHUB_PATH:?GitHub path output file required}"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
    echo 'release-plz installer requires the Linux x86_64 release runner' >&2
    exit 1
fi

install_dir=$(mktemp -d "$RUNNER_TEMP/release-plz.XXXXXX")
trap 'rm -rf -- "$install_dir"' EXIT
archive="$install_dir/release-plz.tar.gz"
curl --fail --location --proto '=https' --proto-redir '=https' --tlsv1.2 \
    --retry 3 --connect-timeout 30 --max-time 120 --output "$archive" \
    "https://github.com/release-plz/release-plz/releases/download/release-plz-v${release_plz_version}/release-plz-x86_64-unknown-linux-musl.tar.gz"
printf '%s  %s\n' "$release_plz_sha256" "$archive" | sha256sum --check
tar -xzf "$archive" -C "$install_dir" release-plz
reported_version=$("$install_dir/release-plz" --version)
if [[ "$reported_version" != "release-plz $release_plz_version" ]]; then
    echo 'unexpected release-plz binary version' >&2
    exit 1
fi
rm -- "$archive"
printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
# GitHub removes RUNNER_TEMP after the job; retain only the verified binary
# for the later release command. Every failed install removes its staging.
trap - EXIT
