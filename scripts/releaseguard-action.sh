#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 || -z "${1:-}" ]]; then
  echo "Usage: releaseguard-action.sh <dataset-path> [test-release-base-url]" >&2
  exit 2
fi

dataset_path="$1"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
action_root="$(cd -- "$script_dir/.." && pwd -P)"
release_version="$(<"$action_root/action-version.txt")"

if [[ ! "$release_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid action release version in action-version.txt: $release_version" >&2
  exit 2
fi

if [[ "${RUNNER_OS:-}" != Linux || "${RUNNER_ARCH:-}" != X64 ]]; then
  echo "ReleaseGuard Action supports Linux x86_64 runners; got ${RUNNER_OS:-unknown}/${RUNNER_ARCH:-unknown}." >&2
  exit 2
fi

if [[ $# -eq 2 ]]; then
  release_base_url="$2"
  if [[ "$release_base_url" != file:///* ]]; then
    echo "The optional test release URL must use file://." >&2
    exit 2
  fi
  curl_options=(--proto '=file' --fail --silent --show-error)
else
  release_base_url="https://github.com/KageRyo/ReleaseGuard/releases/download/$release_version"
  curl_options=(--proto '=https' --proto-redir '=https' --fail --silent --show-error --location --connect-timeout 20 --max-time 120)
fi

archive="releaseguard-${release_version}-linux-x86_64.tar.gz"
temp_parent="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
mkdir -p -- "$temp_parent"
tool_dir="$(mktemp -d "${temp_parent%/}/releaseguard-action.XXXXXX")"
trap 'rm -rf -- "$tool_dir"' EXIT

curl "${curl_options[@]}" --output "$tool_dir/$archive" "${release_base_url%/}/$archive"
curl "${curl_options[@]}" --output "$tool_dir/SHA256SUMS" "${release_base_url%/}/SHA256SUMS"

selected_checksum="$tool_dir/selected.sha256"
: > "$selected_checksum"
matches=0
while IFS= read -r line; do
  [[ "$line" == *"  "* ]] || continue
  checksum="${line%%  *}"
  filename="${line#*  }"
  if [[ "$filename" == "$archive" ]]; then
    if [[ ${#checksum} -ne 64 || "$checksum" == *[!0-9a-fA-F]* ]]; then
      echo "Invalid SHA-256 entry for $archive." >&2
      exit 1
    fi
    matches=$((matches + 1))
    printf '%s  %s\n' "$checksum" "$archive" >> "$selected_checksum"
  fi
done < "$tool_dir/SHA256SUMS"

if [[ "$matches" -ne 1 ]]; then
  echo "Expected exactly one SHA256SUMS entry for $archive; found $matches." >&2
  exit 1
fi

if ! (cd -- "$tool_dir" && sha256sum --check --strict selected.sha256); then
  echo "ReleaseGuard archive checksum verification failed." >&2
  exit 1
fi

tar -tzf "$tool_dir/$archive" > "$tool_dir/archive-files.txt"
LC_ALL=C sort "$tool_dir/archive-files.txt" > "$tool_dir/archive-files.sorted"
printf '%s\n' LICENSE THIRD-PARTY-LICENSES.txt releaseguard | LC_ALL=C sort > "$tool_dir/expected-files.txt"
if ! diff -u "$tool_dir/expected-files.txt" "$tool_dir/archive-files.sorted"; then
  echo "Unexpected files in ReleaseGuard archive." >&2
  exit 1
fi

tar -xzf "$tool_dir/$archive" -C "$tool_dir" -- releaseguard
if [[ ! -f "$tool_dir/releaseguard" || -L "$tool_dir/releaseguard" ]]; then
  echo "ReleaseGuard executable is missing or is not a regular file." >&2
  exit 1
fi
chmod 0755 "$tool_dir/releaseguard"

workspace="${GITHUB_WORKSPACE:?GITHUB_WORKSPACE must be set}"
if [[ ! -d "$workspace" ]]; then
  echo "GitHub workspace directory does not exist: $workspace" >&2
  exit 2
fi
cd -- "$workspace"

set +e
"$tool_dir/releaseguard" validate -- "$dataset_path"
releaseguard_status=$?
set -e
exit "$releaseguard_status"
