#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo "The Action integration test requires Linux x86_64." >&2
  exit 2
fi
export RUNNER_OS=Linux
export RUNNER_ARCH=X64

version="$(<"$repo_root/action-version.txt")"
binary="$repo_root/target/release/releaseguard"
helper="$repo_root/scripts/releaseguard-action.sh"
expected_cli_version="releaseguard ${version#v}"

if [[ ! -x "$binary" ]]; then
  echo "Build the release binary before running this test: cargo build --release --locked" >&2
  exit 2
fi
if [[ "$("$binary" --version)" != "$expected_cli_version" ]]; then
  echo "The CLI version does not match action-version.txt ($version)." >&2
  exit 1
fi

test_root="$(mktemp -d "${TMPDIR:-/tmp}/releaseguard-action-test.XXXXXX")"
trap 'rm -rf -- "$test_root"' EXIT

release_dir="$test_root/releases"
package_dir="$test_root/package"
workspace_dir="$test_root/workspace"
mkdir -p "$release_dir" "$package_dir" "$workspace_dir"

archive="releaseguard-${version}-linux-x86_64.tar.gz"
cp "$binary" "$package_dir/releaseguard"
cp "$repo_root/LICENSE" "$repo_root/THIRD-PARTY-LICENSES.txt" "$package_dir/"
tar -czf "$release_dir/$archive" -C "$package_dir" releaseguard LICENSE THIRD-PARTY-LICENSES.txt
(cd "$release_dir" && sha256sum "$archive" > SHA256SUMS)

cp -R "$repo_root/examples/basic-release" "$workspace_dir/basic dataset"
cp -R "$repo_root/examples/invalid-release" "$workspace_dir/invalid dataset"
mkdir -p "$workspace_dir/config error dataset"
printf 'release: [\n' > "$workspace_dir/config error dataset/release.yaml"
export GITHUB_WORKSPACE="$workspace_dir"
export RUNNER_TEMP="$test_root/runner-temp"
mkdir -p "$RUNNER_TEMP"

release_base_url="file://$release_dir"
bash "$helper" "basic dataset" "$release_base_url"

if bash "$helper" "invalid dataset" "$release_base_url"; then
  echo "The invalid fixture unexpectedly passed Action validation." >&2
  exit 1
else
  validation_status=$?
fi
if [[ "$validation_status" -ne 1 ]]; then
  echo "Expected invalid fixture exit code 1, got $validation_status." >&2
  exit 1
fi

if bash "$helper" "config error dataset" "$release_base_url"; then
  echo "The malformed configuration unexpectedly passed Action validation." >&2
  exit 1
else
  configuration_status=$?
fi
if [[ "$configuration_status" -ne 2 ]]; then
  echo "Expected malformed configuration exit code 2, got $configuration_status." >&2
  exit 1
fi

bad_release_dir="$test_root/bad-release"
mkdir -p "$bad_release_dir"
cp "$release_dir/$archive" "$bad_release_dir/"
read -r digest checksum_name < "$release_dir/SHA256SUMS"
if [[ "$digest" == 0* ]]; then
  bad_first_digit=1
else
  bad_first_digit=0
fi
printf '%s  %s\n' "${bad_first_digit}${digest:1}" "$checksum_name" > "$bad_release_dir/SHA256SUMS"

if output="$(bash "$helper" "basic dataset" "file://$bad_release_dir" 2>&1)"; then
  echo "The Action unexpectedly accepted an archive with a bad checksum." >&2
  exit 1
else
  checksum_status=$?
fi
printf '%s\n' "$output"
if [[ "$checksum_status" -ne 1 || "$output" != *FAILED* ]]; then
  echo "Expected checksum verification to fail before execution (status 1)." >&2
  exit 1
fi

echo "Action integration passed: valid dataset, validation exit code, and checksum rejection."
