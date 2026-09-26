use crate::config::{Config, Release};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub release: Release,
    pub files: BTreeMap<String, Entry>,
}

fn expected_files(config: &Config) -> BTreeMap<String, PathBuf> {
    let mut files = BTreeMap::from([("release.yaml".into(), PathBuf::from("release.yaml"))]);
    files.extend(config.files.values().map(|spec| {
        (
            spec.path.to_string_lossy().replace('\\', "/"),
            spec.path.clone(),
        )
    }));
    files
}

fn file_path(root: &Path, relative: &Path) -> Result<Option<PathBuf>, String> {
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", relative.display())),
    }
    let resolved = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", relative.display()))?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "{}: file resolves outside dataset root",
            relative.display()
        ));
    }
    if !resolved.is_file() {
        return Err(format!("{}: not a regular file", relative.display()));
    }
    Ok(Some(resolved))
}

fn hash(path: &Path) -> Result<Entry, String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        size += read as u64;
    }
    Ok(Entry {
        size,
        sha256: format!("{:x}", digest.finalize()),
    })
}

pub fn generate(root: &Path, config: &Config) -> Result<(), String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let mut files = BTreeMap::new();
    for (name, relative) in expected_files(config) {
        let Some(path) = file_path(&root, &relative)? else {
            return Err(format!("missing configured file: {}", relative.display()));
        };
        files.insert(name, hash(&path)?);
    }
    let manifest = Manifest {
        release: config.release.clone(),
        files,
    };
    let mut output = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    output.push(b'\n');
    let path = root.join("manifest.json");
    check_manifest_path(&path, true)?;
    let mut file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(&output)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

pub struct Verification {
    pub checks: Vec<(String, bool, String)>,
}

impl Verification {
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|(_, passed, _)| *passed)
    }
}

pub fn verify(root: &Path, config: &Config) -> Result<Verification, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let path = root.join("manifest.json");
    check_manifest_path(&path, false)?;
    let content = fs::read(&path).map_err(|e| format!("manifest.json: {e}"))?;
    let manifest: Manifest =
        serde_json::from_slice(&content).map_err(|e| format!("manifest.json: {e}"))?;
    let mut checks = Vec::new();
    let release_matches = manifest.release.name == config.release.name
        && manifest.release.version == config.release.version;
    checks.push((
        "release".into(),
        release_matches,
        if release_matches {
            "Release identity verified."
        } else {
            "Release identity mismatch."
        }
        .into(),
    ));
    let expected_files = expected_files(config);
    for extra in manifest
        .files
        .keys()
        .filter(|path| !expected_files.contains_key(*path))
    {
        checks.push((extra.clone(), false, "Unexpected manifest entry.".into()));
    }
    for (name, relative) in expected_files {
        let Some(expected) = manifest.files.get(&name) else {
            checks.push((name, false, "Missing manifest entry.".into()));
            continue;
        };
        if expected.sha256.len() != 64 || !expected.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("manifest.json: invalid SHA-256 for {name}"));
        }
        let Some(path) = file_path(&root, &relative)? else {
            checks.push((name, false, "Missing file.".into()));
            continue;
        };
        let current = hash(&path)?;
        let passed = current.size == expected.size && current.sha256 == expected.sha256;
        checks.push((
            name,
            passed,
            if passed {
                "Integrity verified."
            } else {
                "Size or checksum mismatch."
            }
            .into(),
        ));
    }
    Ok(Verification { checks })
}

fn check_manifest_path(path: &Path, allow_missing: bool) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("manifest.json must not be a symbolic link".into())
        }
        Ok(metadata) if !metadata.is_file() => Err("manifest.json must be a regular file".into()),
        Ok(_) => Ok(()),
        Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("manifest.json: {error}")),
    }
}
