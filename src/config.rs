use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub release: Release,
    pub files: BTreeMap<String, FileSpec>,
    #[serde(default)]
    pub checks: Vec<Check>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileSpec {
    pub path: PathBuf,
    pub format: FileFormat,
    pub schema: BTreeMap<String, FieldSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileFormat {
    Csv,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    #[serde(rename = "type")]
    pub kind: FieldType,
    #[serde(default)]
    pub nullable: bool,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Datetime,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Check {
    Unique {
        id: String,
        field: String,
    },
    ForeignKey {
        id: String,
        from: String,
        to: String,
    },
    TemporalOrder {
        id: String,
        left: String,
        operator: Operator,
        right: String,
        join: Option<Join>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Join {
    pub left: String,
    pub right: String,
}

#[derive(Debug, Deserialize)]
pub enum Operator {
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Ge,
}

impl Check {
    pub fn id(&self) -> &str {
        match self {
            Self::Unique { id, .. }
            | Self::ForeignKey { id, .. }
            | Self::TemporalOrder { id, .. } => id,
        }
    }
}

pub fn load(root: &Path) -> Result<Config, String> {
    let path = root.join("release.yaml");
    let content = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let config: Config =
        serde_yaml::from_str(&content).map_err(|e| format!("release.yaml: {e}"))?;
    config.check()?;
    Ok(config)
}

impl Config {
    fn check(&self) -> Result<(), String> {
        if self.release.name.trim().is_empty() || self.release.version.trim().is_empty() {
            return Err("release name and version must be nonempty".into());
        }
        if self.files.is_empty() {
            return Err("files must contain at least one CSV file".into());
        }
        let mut paths = HashSet::new();
        let mut ids = HashSet::new();
        for (alias, file) in &self.files {
            if alias.is_empty() || alias.contains('.') || alias.contains('/') {
                return Err(format!("invalid file alias: {alias}"));
            }
            if file.schema.is_empty() {
                return Err(format!("{alias}: schema must contain at least one field"));
            }
            if file.path.as_os_str().is_empty()
                || file.path.to_string_lossy().contains('\\')
                || file
                    .path
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(format!(
                    "{alias}: path must be a relative path without . or .."
                ));
            }
            if file.path == Path::new("manifest.json") {
                return Err(format!("{alias}: manifest.json cannot be a dataset file"));
            }
            if !paths.insert(file.path.clone()) {
                return Err(format!("duplicate file path: {}", file.path.display()));
            }
            for field in file.schema.keys() {
                if field.trim().is_empty() || field.contains('.') {
                    return Err(format!("{alias}: invalid field name: {field}"));
                }
            }
            ids.insert(format!("{alias}-schema"));
        }
        for check in &self.checks {
            if check.id().trim().is_empty() || !ids.insert(check.id().to_owned()) {
                return Err(format!("duplicate or empty check id: {}", check.id()));
            }
            match check {
                Check::Unique { field, .. } => {
                    self.field(field)?;
                }
                Check::ForeignKey { from, to, .. } => {
                    self.field(from)?;
                    self.field(to)?;
                }
                Check::TemporalOrder {
                    left, right, join, ..
                } => {
                    let (left_file, left_field) = self.field(left)?;
                    let (right_file, right_field) = self.field(right)?;
                    if left_field.kind != FieldType::Datetime
                        || right_field.kind != FieldType::Datetime
                    {
                        return Err(format!(
                            "{}: temporal fields must have datetime type",
                            check.id()
                        ));
                    }
                    if left_file != right_file && join.is_none() {
                        return Err(format!(
                            "{}: cross-file temporal check requires join",
                            check.id()
                        ));
                    }
                    if let Some(join) = join {
                        let (jl, _) = self.field(&join.left)?;
                        let (jr, _) = self.field(&join.right)?;
                        if jl != left_file || jr != right_file {
                            return Err(format!(
                                "{}: join fields must match left and right files",
                                check.id()
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn field(&self, reference: &str) -> Result<(&str, &FieldSpec), String> {
        let (alias, field) = reference
            .split_once('.')
            .ok_or_else(|| format!("invalid field reference: {reference}"))?;
        if field.contains('.') {
            return Err(format!("invalid field reference: {reference}"));
        }
        let file = self
            .files
            .get(alias)
            .ok_or_else(|| format!("unknown file alias: {alias}"))?;
        let spec = file
            .schema
            .get(field)
            .ok_or_else(|| format!("unknown schema field: {reference}"))?;
        Ok((self.files.get_key_value(alias).unwrap().0.as_str(), spec))
    }
}
