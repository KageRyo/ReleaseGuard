use crate::config::{Check, Config, FieldType, Operator, Release};
use crate::dataset::{self, Table};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Passed,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub id: String,
    pub status: Status,
    pub affected_rows: usize,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub release: Release,
    pub status: Status,
    pub checks: Vec<CheckResult>,
}

fn result(
    id: String,
    count: usize,
    message: impl Into<String>,
    examples: Vec<String>,
) -> CheckResult {
    CheckResult {
        id,
        status: if count == 0 {
            Status::Passed
        } else {
            Status::Failed
        },
        affected_rows: count,
        message: message.into(),
        examples,
    }
}

fn unavailable(id: String, reason: impl Into<String>) -> CheckResult {
    result(id, 1, reason, vec![])
}

fn table<'a>(
    tables: &'a BTreeMap<String, Option<Table>>,
    reference: &str,
) -> Option<(&'a Table, &'a str)> {
    let (alias, field) = reference.split_once('.')?;
    // The field name originates from the checked configuration; retain the same lifetime as the table.
    let table = tables.get(alias)?.as_ref()?;
    let field = table.column_name(field)?;
    Some((table, field))
}

fn push_example(examples: &mut Vec<String>, value: &str) {
    if examples.len() < 3 && !examples.iter().any(|e| e == value) {
        examples.push(value.to_owned());
    }
}

fn valid_value(kind: FieldType, value: &str) -> bool {
    match kind {
        FieldType::String => true,
        FieldType::Integer => value.parse::<i64>().is_ok(),
        FieldType::Float => value.parse::<f64>().is_ok_and(f64::is_finite),
        FieldType::Boolean => matches!(value, "true" | "false"),
        FieldType::Datetime => parse_time(value).is_some(),
    }
}

fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

pub fn run(root: &Path, config: &Config) -> Result<Report, String> {
    let tables = dataset::load(root, config)?;
    let mut checks = Vec::new();
    for (alias, spec) in &config.files {
        let id = format!("{alias}-schema");
        let Some(table) = tables.get(alias).and_then(Option::as_ref) else {
            checks.push(unavailable(
                id,
                format!("Missing file: {}", spec.path.display()),
            ));
            continue;
        };
        let mut count = 0;
        let mut examples = Vec::new();
        for (field, field_spec) in &spec.schema {
            if !table.has_column(field) {
                count += 1;
                push_example(&mut examples, &format!("missing column {field}"));
                continue;
            }
            for (index, row) in table.rows().iter().enumerate() {
                let value = table.value(row, field).unwrap_or("");
                if value.is_empty() && !field_spec.nullable
                    || !value.is_empty() && !valid_value(field_spec.kind, value)
                {
                    count += 1;
                    push_example(&mut examples, &format!("row {}: {field}", index + 2));
                }
            }
        }
        checks.push(result(
            id,
            count,
            if count == 0 {
                "Schema valid."
            } else {
                "Schema violations found."
            },
            examples,
        ));
    }
    for check in &config.checks {
        checks.push(match check {
            Check::Unique { id, field } => unique(id, field, &tables),
            Check::ForeignKey { id, from, to } => foreign_key(id, from, to, &tables),
            Check::TemporalOrder {
                id,
                left,
                operator,
                right,
                join,
            } => temporal(id, left, operator, right, join.as_ref(), &tables),
        });
    }
    let status = if checks.iter().all(|c| c.status == Status::Passed) {
        Status::Passed
    } else {
        Status::Failed
    };
    Ok(Report {
        release: config.release.clone(),
        status,
        checks,
    })
}

fn unique(id: &str, field: &str, tables: &BTreeMap<String, Option<Table>>) -> CheckResult {
    let Some((table, name)) = table(tables, field) else {
        return unavailable(id.into(), format!("Cannot read {field}."));
    };
    let mut seen = HashSet::new();
    let mut count = 0;
    let mut examples = Vec::new();
    for row in table.rows() {
        let value = table.value(row, name).unwrap_or("");
        if !value.is_empty() && !seen.insert(value) {
            count += 1;
            push_example(&mut examples, value);
        }
    }
    result(
        id.into(),
        count,
        if count == 0 {
            "Values are unique.".into()
        } else {
            format!("{count} duplicate values found.")
        },
        examples,
    )
}

fn foreign_key(
    id: &str,
    from: &str,
    to: &str,
    tables: &BTreeMap<String, Option<Table>>,
) -> CheckResult {
    let Some((source, source_field)) = table(tables, from) else {
        return unavailable(id.into(), format!("Cannot read {from}."));
    };
    let Some((target, target_field)) = table(tables, to) else {
        return unavailable(id.into(), format!("Cannot read {to}."));
    };
    let values: HashSet<_> = target
        .rows()
        .iter()
        .filter_map(|row| target.value(row, target_field))
        .filter(|v| !v.is_empty())
        .collect();
    let mut count = 0;
    let mut examples = Vec::new();
    for row in source.rows() {
        let value = source.value(row, source_field).unwrap_or("");
        if !value.is_empty() && !values.contains(value) {
            count += 1;
            push_example(&mut examples, value);
        }
    }
    result(
        id.into(),
        count,
        if count == 0 {
            "All references exist.".into()
        } else {
            format!("{count} missing references found.")
        },
        examples,
    )
}

fn temporal(
    id: &str,
    left: &str,
    operator: &Operator,
    right: &str,
    join: Option<&crate::config::Join>,
    tables: &BTreeMap<String, Option<Table>>,
) -> CheckResult {
    let Some((left_table, left_field)) = table(tables, left) else {
        return unavailable(id.into(), format!("Cannot read {left}."));
    };
    let Some((right_table, right_field)) = table(tables, right) else {
        return unavailable(id.into(), format!("Cannot read {right}."));
    };
    let mut count = 0;
    let mut examples = Vec::new();
    if let Some(join) = join {
        let Some((_, left_key)) = table(tables, &join.left) else {
            return unavailable(id.into(), format!("Cannot read {}.", join.left));
        };
        let Some((_, right_key)) = table(tables, &join.right) else {
            return unavailable(id.into(), format!("Cannot read {}.", join.right));
        };
        let mut right_rows: HashMap<&str, Vec<&Vec<String>>> = HashMap::new();
        for row in right_table.rows() {
            let key = right_table.value(row, right_key).unwrap_or("");
            if !key.is_empty() {
                right_rows.entry(key).or_default().push(row);
            }
        }
        for row in left_table.rows() {
            let key = left_table.value(row, left_key).unwrap_or("");
            let matches = right_rows.get(key);
            let good = matches.is_some_and(|rows| {
                rows.len() == 1
                    && time_order(
                        left_table.value(row, left_field).unwrap_or(""),
                        right_table.value(rows[0], right_field).unwrap_or(""),
                        operator,
                    )
            });
            if !good {
                count += 1;
                push_example(
                    &mut examples,
                    if key.is_empty() {
                        "<empty join key>"
                    } else {
                        key
                    },
                );
            }
        }
    } else {
        for (index, row) in left_table.rows().iter().enumerate() {
            if !time_order(
                left_table.value(row, left_field).unwrap_or(""),
                right_table.value(row, right_field).unwrap_or(""),
                operator,
            ) {
                count += 1;
                push_example(&mut examples, &format!("row {}", index + 2));
            }
        }
    }
    result(
        id.into(),
        count,
        if count == 0 {
            "Temporal order valid.".into()
        } else {
            format!("{count} temporal violations found.")
        },
        examples,
    )
}

fn time_order(left: &str, right: &str, operator: &Operator) -> bool {
    let (Some(left), Some(right)) = (parse_time(left), parse_time(right)) else {
        return false;
    };
    match operator {
        Operator::Lt => left < right,
        Operator::Le => left <= right,
        Operator::Gt => left > right,
        Operator::Ge => left >= right,
    }
}
