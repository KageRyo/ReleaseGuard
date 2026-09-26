# ReleaseGuard

[![CI](https://github.com/KageRyo/ReleaseGuard/actions/workflows/ci.yml/badge.svg)](https://github.com/KageRyo/ReleaseGuard/actions/workflows/ci.yml) [![Latest release](https://img.shields.io/github/v/release/KageRyo/ReleaseGuard?display_name=tag&sort=semver)](https://github.com/KageRyo/ReleaseGuard/releases) [![License](https://img.shields.io/github/license/KageRyo/ReleaseGuard.svg)](LICENSE)

> ReleaseGuard is a standalone dataset release gate for CI pipelines.

ReleaseGuard checks a candidate CSV dataset against a small declarative `release.yaml`, then returns PASS or FAIL before publication. It checks file schemas, uniqueness, references, timestamp order, and SHA-256 integrity. It is a local command line program with no Python runtime, database, service, or network connection during validation. It is not an ETL tool, data catalog, lineage platform, ML validator, or replacement for Great Expectations or DVC.

## GitHub Actions

Use the composite Action in a dataset repository after checking out its files. Pin the Action to an immutable version; the Action tag and downloaded CLI release use the same version. The Action downloads the Linux x86_64 archive over HTTPS and verifies it against `SHA256SUMS` before extraction and execution. It does not require Rust, Python, or Node.js in the consumer repository.

```yaml
name: Dataset gate
on:
  pull_request:
jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: KageRyo/ReleaseGuard@v0.2.1
        with:
          path: .
```

`path` is relative to the checked-out workspace and defaults to `.`. The selected directory must contain its own `release.yaml`. The Action currently runs `releaseguard validate` on Linux x86_64 (`ubuntu-latest`); use the standalone CLI for `manifest`, `verify`, other platforms, or advanced options.

## Install

Download the archive for your platform from [GitHub Releases](https://github.com/KageRyo/ReleaseGuard/releases). The v0.2.1 assets are named `releaseguard-v0.2.1-linux-x86_64.tar.gz`, `releaseguard-v0.2.1-windows-x86_64.zip`, and `releaseguard-v0.2.1-macos-aarch64.tar.gz`. Each archive includes the executable and its license notices; `SHA256SUMS` covers all three archives.

The Linux x86_64 binary targets GNU/Linux and is built on Ubuntu 22.04. The macOS binary supports Apple silicon (arm64).

On Linux x86_64 or macOS arm64, extract the matching `.tar.gz` archive and put the executable on your `PATH`. Use the `linux-x86_64` asset on Linux and `macos-aarch64` on Apple silicon.

```sh
tar -xzf releaseguard-v0.2.1-linux-x86_64.tar.gz
install -m 0755 releaseguard "$HOME/.local/bin/releaseguard"
releaseguard --help
```

On Windows x86_64, extract the `.zip` archive, add the extracted directory to `PATH`, then run `releaseguard.exe --help`. No Rust toolchain is needed to use a release binary.

For development from source, install Rust and run `cargo build --release --locked`. The executable is written to `target/release/releaseguard` on Unix or `target/release/releaseguard.exe` on Windows.

## Quick start

```sh
releaseguard init ./dataset
# Create ./dataset/data.csv with an id column, then edit release.yaml for your release.
releaseguard validate ./dataset
releaseguard validate ./dataset --format json
releaseguard manifest ./dataset
releaseguard verify ./dataset
```

`init` creates only `release.yaml` and refuses to overwrite one that exists. The starter declares `data.csv` with a required `id` and a uniqueness check. The working synthetic example is in `examples/basic-release` and includes a verifiable manifest; `examples/invalid-release` shows failed rules. Try `releaseguard validate examples/basic-release`, `releaseguard verify examples/basic-release`, and `releaseguard validate examples/invalid-release`.

## `release.yaml` v0.1

```yaml
release:
  name: example-dataset
  version: 1.0.0
files:
  events:
    path: data/events.csv
    format: csv
    schema:
      event_id: { type: string, nullable: false }
      occurred_at: { type: datetime, nullable: false }
  actions:
    path: data/actions.csv
    format: csv
    schema:
      action_id: { type: string, nullable: false }
      event_id: { type: string, nullable: false }
      created_at: { type: datetime, nullable: false }
checks:
  - id: event-id-unique
    type: unique
    field: events.event_id
  - id: action-event-reference
    type: foreign_key
    from: actions.event_id
    to: events.event_id
  - id: action-after-event
    type: temporal_order
    left: actions.created_at
    operator: ">="
    right: events.occurred_at
    join: { left: actions.event_id, right: events.event_id }
```

`files` is a nonempty map of aliases to relative CSV paths and nonempty field schemas. Use `/` as the path separator; absolute paths, `.`/`..` components, and backslashes are rejected. `release.yaml`, `manifest.json`, and paths beneath either are reserved. Paths stay inside the dataset directory, including through symbolic links. Every configured schema field must exist as a CSV column; extra columns are allowed. Empty CSV cells are null. `nullable` defaults to `false`. Supported types are `string`, signed 64-bit `integer`, finite `float`, lowercase `true`/`false` `boolean`, and RFC 3339 `datetime` with an explicit UTC offset or `Z`. Datetimes compare as instants, so different offsets representing the same time compare equal. An empty string is null even for `string` fields. CSV headers and values are case sensitive.

Every file receives an automatic `<alias>-schema` check. Explicit check IDs must be unique and cannot collide with those automatic IDs. Field references use `alias.column` and must name a configured schema field. `unique` counts repeated non-null values after the first occurrence. `foreign_key` requires each non-null source value to occur in the target field; null targets are ignored. `temporal_order` supports `<`, `<=`, `>`, and `>=`. Without `join`, it compares fields in the same row of one file. Different files require `join` with equality key fields from the respective files. Every left row must match exactly one right row, and the timestamp comparison must hold. Missing or ambiguous join matches and null or unparseable timestamp values fail the temporal check. Add separate `unique` and `foreign_key` checks if those constraints also matter independently.

Configuration errors such as unknown aliases, invalid field references, unsupported types, unsafe paths, and missing cross-file joins exit with code 2. A missing configured CSV file is a validation failure with code 1; a malformed CSV is an input error with code 2.

## Commands, output, and integrity

`validate` defaults to concise text for people and CI logs. `--format json` prints a stable object with `release`, overall `status`, and ordered `checks`. Each check has `id`, `status`, `affected_rows`, `message`, and up to three `examples` on failure. Failure counts are per invalid field value for schema checks and per affected source row for other checks. JSON is printed on stdout, and configuration/execution errors go to stderr.

`manifest` writes deterministic `manifest.json` containing release identity and a sorted `files` map with byte size and SHA-256 for both `release.yaml` and every configured data file. No timestamp is added. `verify` checks the current `release.yaml` bytes as well as release identity and each configured file's size and SHA-256, so post-manifest edits to the release specification fail verification. Regenerate manifests created by earlier pre-release versions because they do not include the specification hash. Check in the manifest with a dataset release if the release workflow must verify it later. A manifest missing from disk or malformed JSON exits with code 2; changed or missing tracked files or a changed release specification exit with code 1.

Exit codes are `0` for success, `1` for dataset validation or integrity failure, and `2` for configuration, malformed input, or execution errors. All commands are noninteractive.

## GitHub Actions release gate

The Action runs `validate` and passes ReleaseGuard's exit code to the job. Use the standalone CLI for `manifest`, `verify`, or other commands. This repository's own CI runs formatting, Clippy, tests, a release build, and an Action integration smoke test on pushes and pull requests. Configure branch protection in a dataset repository if merges must require the validation job.

## Limits and roadmap

The CLI reads local CSV files into memory; it does not support remote data, Parquet, JSON Lines, databases, transformations, statistical profiling, or automatic publishing. Manifest files are integrity records, not signed attestations. Linux x86_64, Windows x86_64, and macOS arm64 release binaries are distributed as unsigned archives.

## License

ReleaseGuard is licensed under [Apache-2.0](LICENSE). Release archives also include [third-party license notices](THIRD-PARTY-LICENSES.txt) for bundled dependencies.
