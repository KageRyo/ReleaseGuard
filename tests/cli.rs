use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn fixture(name: &str) -> TempDir {
    let temp = TempDir::new().unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name);
    for relative in ["release.yaml", "data/events.csv", "data/actions.csv"] {
        let destination = temp.path().join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source.join(relative), destination).unwrap();
    }
    temp
}

fn command(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_releaseguard"))
        .args(args)
        .output()
        .unwrap()
}

fn validate(path: &Path) -> (i32, Value) {
    let output = command(&["validate", path.to_str().unwrap(), "--format", "json"]);
    let json = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    (output.status.code().unwrap(), json)
}

fn config_edit(path: &Path, from: &str, to: &str) {
    let config = path.join("release.yaml");
    let original = fs::read_to_string(&config).unwrap();
    assert!(original.contains(from));
    fs::write(config, original.replace(from, to)).unwrap();
}

#[test]
fn valid_release_and_json_output() {
    let temp = fixture("basic-release");
    let (code, json) = validate(temp.path());
    assert_eq!(code, 0);
    assert_eq!(json["release"]["name"], "example-dataset");
    assert_eq!(json["status"], "passed");
    assert_eq!(json["checks"].as_array().unwrap().len(), 5);
    assert!(json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|check| check["status"] == "passed"));
    let text = command(&["validate", temp.path().to_str().unwrap()]);
    assert_eq!(text.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&text.stdout).contains("Validation PASSED."));
}

#[test]
fn invalid_example_covers_schema_unique_foreign_key_and_temporal_join() {
    let temp = fixture("invalid-release");
    let (code, json) = validate(temp.path());
    assert_eq!(code, 1);
    assert_eq!(json["status"], "failed");
    let checks = json["checks"].as_array().unwrap();
    for id in [
        "events-schema",
        "actions-schema",
        "event-id-unique",
        "action-event-reference",
        "action-after-event",
    ] {
        let check = checks.iter().find(|check| check["id"] == id).unwrap();
        assert_eq!(check["status"], "failed", "{id}");
        assert!(check["affected_rows"].as_u64().unwrap() > 0);
    }
    assert_eq!(
        checks
            .iter()
            .find(|c| c["id"] == "action-event-reference")
            .unwrap()["examples"][0],
        "E999"
    );
}

#[test]
fn missing_column_null_and_primitive_types_fail_schema() {
    let temp = fixture("basic-release");
    config_edit(temp.path(), "      active: { type: boolean, nullable: false }", "      active: { type: boolean, nullable: false }\n      absent: { type: string, nullable: false }");
    fs::write(temp.path().join("data/actions.csv"), "action_id,event_id,created_at,priority,ratio,active\nA001,E001,bad-time,nope,NaN,yes\nA002,,2026-01-02T10:00:00+08:00,2,,false\n").unwrap();
    let (code, json) = validate(temp.path());
    assert_eq!(code, 1);
    let check = json["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "actions-schema")
        .unwrap();
    assert_eq!(check["status"], "failed");
    assert!(check["affected_rows"].as_u64().unwrap() >= 6);
    assert!(check["examples"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e == "missing column absent"));
}

#[test]
fn malformed_config_unknown_alias_and_unsafe_path_exit_two() {
    let temp = fixture("basic-release");
    config_edit(
        temp.path(),
        "field: events.event_id",
        "field: unknown.event_id",
    );
    assert_eq!(
        command(&["validate", temp.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    config_edit(temp.path(), "unknown.event_id", "events.event_id");
    config_edit(temp.path(), "path: data/events.csv", "path: ../events.csv");
    assert_eq!(
        command(&["validate", temp.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    config_edit(temp.path(), "path: ../events.csv", "path: manifest.json");
    assert_eq!(
        command(&["manifest", temp.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    fs::write(temp.path().join("release.yaml"), "release: [").unwrap();
    assert_eq!(
        command(&["validate", temp.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn missing_file_is_validation_failure_and_bad_csv_is_execution_error() {
    let temp = fixture("basic-release");
    fs::remove_file(temp.path().join("data/events.csv")).unwrap();
    let (code, json) = validate(temp.path());
    assert_eq!(code, 1);
    assert_eq!(json["checks"][1]["status"], "failed");
    fs::write(
        temp.path().join("data/events.csv"),
        "event_id,occurred_at\nE001\n",
    )
    .unwrap();
    assert_eq!(
        command(&["validate", temp.path().to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn temporal_operators_compare_same_file_rows() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("release.yaml"), "release: { name: times, version: v1 }\nfiles:\n  times:\n    path: times.csv\n    format: csv\n    schema:\n      earlier: { type: datetime, nullable: false }\n      later: { type: datetime, nullable: false }\nchecks:\n  - { id: lt, type: temporal_order, left: times.earlier, operator: '<', right: times.later }\n  - { id: le, type: temporal_order, left: times.earlier, operator: '<=', right: times.later }\n  - { id: gt, type: temporal_order, left: times.later, operator: '>', right: times.earlier }\n  - { id: ge, type: temporal_order, left: times.later, operator: '>=', right: times.earlier }\n").unwrap();
    fs::write(
        temp.path().join("times.csv"),
        "earlier,later\n2026-01-01T00:00:00Z,2026-01-01T01:00:00+01:00\n",
    )
    .unwrap();
    let (code, json) = validate(temp.path());
    assert_eq!(code, 1);
    let checks = json["checks"].as_array().unwrap();
    assert_eq!(checks.iter().filter(|c| c["status"] == "failed").count(), 2);
    assert_eq!(
        checks.iter().find(|c| c["id"] == "le").unwrap()["status"],
        "passed"
    );
    assert_eq!(
        checks.iter().find(|c| c["id"] == "ge").unwrap()["status"],
        "passed"
    );
}

#[test]
fn manifest_is_deterministic_and_verify_detects_changes_and_missing_files() {
    let temp = fixture("basic-release");
    let path = temp.path().to_str().unwrap();
    assert_eq!(command(&["manifest", path]).status.code(), Some(0));
    let first = fs::read(temp.path().join("manifest.json")).unwrap();
    assert_eq!(command(&["manifest", path]).status.code(), Some(0));
    assert_eq!(first, fs::read(temp.path().join("manifest.json")).unwrap());
    let manifest: Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(
        manifest["files"]["data/events.csv"]["sha256"],
        "28970a1fe535f118259ea256d3c9bb7d25e71985a9c50b38ceccc9ffcf93d406"
    );
    assert_eq!(command(&["verify", path]).status.code(), Some(0));
    fs::write(
        temp.path().join("data/events.csv"),
        "event_id,occurred_at\nE001,changed\n",
    )
    .unwrap();
    assert_eq!(command(&["verify", path]).status.code(), Some(1));
    fs::remove_file(temp.path().join("data/events.csv")).unwrap();
    assert_eq!(command(&["verify", path]).status.code(), Some(1));
}

#[cfg(unix)]
#[test]
fn manifest_symlink_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let dataset = fixture("basic-release");
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("manifest-target.json");
    fs::write(&target, "leave this file alone").unwrap();
    symlink(&target, dataset.path().join("manifest.json")).unwrap();
    let path = dataset.path().to_str().unwrap();

    assert_eq!(command(&["manifest", path]).status.code(), Some(2));
    assert_eq!(command(&["verify", path]).status.code(), Some(2));
    assert_eq!(fs::read_to_string(target).unwrap(), "leave this file alone");
}

#[test]
fn init_creates_usable_config_without_overwriting() {
    let temp = TempDir::new().unwrap();
    let target = temp.path().join("dataset");
    let path = target.to_str().unwrap();
    assert_eq!(command(&["init", path]).status.code(), Some(0));
    let original = fs::read(target.join("release.yaml")).unwrap();
    assert_eq!(command(&["init", path]).status.code(), Some(2));
    assert_eq!(original, fs::read(target.join("release.yaml")).unwrap());
    fs::write(target.join("data.csv"), "id\nA001\n").unwrap();
    assert_eq!(command(&["validate", path]).status.code(), Some(0));
}
