use crate::config::Config;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

pub struct Table {
    columns: HashMap<String, usize>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn has_column(&self, field: &str) -> bool {
        self.columns.contains_key(field)
    }
    pub fn column_name<'a>(&'a self, field: &str) -> Option<&'a str> {
        self.columns
            .get_key_value(field)
            .map(|(name, _)| name.as_str())
    }
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
    pub fn value<'a>(&self, row: &'a [String], field: &str) -> Option<&'a str> {
        self.columns
            .get(field)
            .and_then(|i| row.get(*i))
            .map(String::as_str)
    }
}

pub fn load(root: &Path, config: &Config) -> Result<BTreeMap<String, Option<Table>>, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("{}: {e}", root.display()))?;
    let mut tables = BTreeMap::new();
    for (alias, spec) in &config.files {
        match spec.format {
            crate::config::FileFormat::Csv => {}
        }
        let path = root.join(&spec.path);
        if !path.exists() {
            tables.insert(alias.clone(), None);
            continue;
        }
        let resolved = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !resolved.starts_with(&root) {
            return Err(format!(
                "{}: file resolves outside dataset root",
                spec.path.display()
            ));
        }
        if !fs::metadata(&resolved)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .is_file()
        {
            return Err(format!("{}: not a regular file", spec.path.display()));
        }
        let mut reader = csv::ReaderBuilder::new()
            .from_path(&resolved)
            .map_err(|e| format!("{}: {e}", spec.path.display()))?;
        let headers = reader
            .headers()
            .map_err(|e| format!("{}: {e}", spec.path.display()))?;
        let mut columns = HashMap::new();
        for (index, field) in headers.iter().enumerate() {
            if columns.insert(field.to_owned(), index).is_some() {
                return Err(format!(
                    "{}: duplicate CSV column: {field}",
                    spec.path.display()
                ));
            }
        }
        let rows = reader
            .records()
            .map(|record| {
                record
                    .map(|r| r.iter().map(str::to_owned).collect())
                    .map_err(|e| format!("{}: {e}", spec.path.display()))
            })
            .collect::<Result<Vec<Vec<String>>, String>>()?;
        tables.insert(alias.clone(), Some(Table { columns, rows }));
    }
    Ok(tables)
}
