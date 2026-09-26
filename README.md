# ReleaseGuard

> ReleaseGuard is a standalone dataset release gate for CI pipelines.

ReleaseGuard checks a candidate CSV dataset against a small declarative `release.yaml`, then returns PASS or FAIL before publication. It checks file schemas, uniqueness, references, timestamp order, and SHA-256 integrity. It is a local command line program with no Python runtime, database, service, or network connection during validation. It is not an ETL tool, data catalog, lineage platform, ML validator, or replacement for Great Expectations or DVC.

## Build and quick start

Install a Rust toolchain and run `cargo build --release --locked`. The standalone executable is `target/release/releaseguard` on Unix or `target/release/releaseguard.exe` on Windows. Copy that executable into `PATH` or use it directly.

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

`files` is a nonempty map of aliases to relative CSV paths and nonempty field schemas. Use `/` as the path separator; absolute paths, `.`/`..` components, and backslashes are rejected. `manifest.json` and paths beneath it are reserved. Paths stay inside the dataset directory, including through symbolic links. Every configured schema field must exist as a CSV column; extra columns are allowed. Empty CSV cells are null. `nullable` defaults to `false`. Supported types are `string`, signed 64-bit `integer`, finite `float`, lowercase `true`/`false` `boolean`, and RFC 3339 `datetime` with an explicit UTC offset or `Z`. Datetimes compare as instants, so different offsets representing the same time compare equal. An empty string is null even for `string` fields. CSV headers and values are case sensitive.

Every file receives an automatic `<alias>-schema` check. Explicit check IDs must be unique and cannot collide with those automatic IDs. Field references use `alias.column` and must name a configured schema field. `unique` counts repeated non-null values after the first occurrence. `foreign_key` requires each non-null source value to occur in the target field; null targets are ignored. `temporal_order` supports `<`, `<=`, `>`, and `>=`. Without `join`, it compares fields in the same row of one file. Different files require `join` with equality key fields from the respective files. Every left row must match exactly one right row, and the timestamp comparison must hold. Missing or ambiguous join matches and null or unparseable timestamp values fail the temporal check. Add separate `unique` and `foreign_key` checks if those constraints also matter independently.

Configuration errors such as unknown aliases, invalid field references, unsupported types, unsafe paths, and missing cross-file joins exit with code 2. A missing configured CSV file is a validation failure with code 1; a malformed CSV is an input error with code 2.

## Commands, output, and integrity

`validate` defaults to concise text for people and CI logs. `--format json` prints a stable object with `release`, overall `status`, and ordered `checks`. Each check has `id`, `status`, `affected_rows`, `message`, and up to three `examples` on failure. Failure counts are per invalid field value for schema checks and per affected source row for other checks. JSON is printed on stdout, and configuration/execution errors go to stderr.

`manifest` writes deterministic `manifest.json` containing release identity and, for each configured file, its relative path, byte size, and SHA-256. File entries are sorted by path and no timestamp is added. Generate it after successful validation. `verify` compares the current release identity and configured file set with the manifest, then checks each file's size and SHA-256. Check in the manifest with a dataset release if the release workflow must verify it later. A manifest missing from disk or malformed JSON exits with code 2; changed or missing tracked files exit with code 1.

Exit codes are `0` for success, `1` for dataset validation or integrity failure, and `2` for configuration, malformed input, or execution errors. All commands are noninteractive.

## GitHub Actions release gate

Build ReleaseGuard from a pinned commit or install a published binary for the runner platform. The example below uses a pinned source revision; replace `<releaseguard-commit-sha>` with a real commit before use. Validation runs on pull requests. Release tags run both validation and manifest verification before any publishing step.

```yaml
name: Dataset gate
on:
  pull_request:
  push:
    tags: ['v*']
jobs:
  gate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          path: dataset
      - uses: actions/checkout@v4
        with:
          repository: KageRyo/ReleaseGuard
          ref: <releaseguard-commit-sha>
          path: releaseguard-src
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release --locked --manifest-path releaseguard-src/Cargo.toml
      - run: releaseguard-src/target/release/releaseguard validate dataset
      - if: startsWith(github.ref, 'refs/tags/')
        run: releaseguard-src/target/release/releaseguard verify dataset
```

This repository's own CI runs formatting, Clippy, tests, and a release build on pushes and pull requests. A failed gate blocks the job through its nonzero exit code; configure branch protection in the dataset repository if merges must require the job.

## Limits and roadmap

v0.1 reads local CSV files into memory; it does not support remote data, Parquet, JSON Lines, databases, transformations, statistical profiling, or automatic publishing. Manifest files are integrity records, not signed attestations. Future versions may add other local formats and prebuilt Linux, Windows, and macOS binaries when needed.
