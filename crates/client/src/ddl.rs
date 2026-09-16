//! The tables are the schema record. Reconciliation makes them match the compiled schema or fails.
use crate::store::ClientStore;
use ahead_core::{
    FieldDescriptor, ModelDescriptor, Result, ScalarType, Schema, ValueType, invalid,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub const FRAMEWORK_TABLES: &[&str] = &[
    "ahead_client",
    "ahead_record",
    "ahead_subscription",
    "ahead_mutation",
    "ahead_mutation_operation",
    "ahead_mutation_dependency",
    "ahead_mutation_prerequisite",
    "ahead_rejection",
];

/// Framework tables an earlier layout kept and this one refuses to open:
/// channel claims owned records and push checkpoints settled batches, both
/// replaced by receipt completion ([#55](https://github.com/zanminwang/ahead/issues/55)).
pub const LEGACY_TABLES: &[&str] = &["ahead_claim", "ahead_push_checkpoint"];

/// `ahead_client` columns this layout requires beyond the original ones. A
/// database created before they existed holds pending work under the old
/// contract; it is refused, never converted or wiped.
const CLIENT_COLUMNS: &[&str] = &["last_completed_push", "push_models"];
/// `ahead_mutation` columns this layout requires: `diverged` marks a queued
/// mutation whose replay failed over new authority
/// ([#122](https://github.com/zanminwang/ahead/issues/122)).
const MUTATION_COLUMNS: &[&str] = &["diverged"];

pub const FRAMEWORK_DDL: &str = "
CREATE TABLE IF NOT EXISTS ahead_client (
  client_id    TEXT PRIMARY KEY,
  next_ordinal INTEGER NOT NULL,
  next_push    INTEGER NOT NULL,
  generation   INTEGER NOT NULL,
  last_completed_push INTEGER NOT NULL DEFAULT 0,
  push_models  TEXT
);
CREATE TABLE IF NOT EXISTS ahead_record (
  model TEXT NOT NULL, identity TEXT NOT NULL, stamp INTEGER NOT NULL,
  PRIMARY KEY (model, identity)
);
CREATE TABLE IF NOT EXISTS ahead_subscription (
  channel TEXT PRIMARY KEY, cursor INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ahead_mutation (
  ordinal INTEGER PRIMARY KEY, name TEXT NOT NULL, version INTEGER NOT NULL, push INTEGER,
  diverged INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS ahead_mutation_operation (
  ordinal INTEGER NOT NULL REFERENCES ahead_mutation(ordinal) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('wire','companion','effect')),
  model TEXT NOT NULL, identity TEXT NOT NULL,
  op TEXT NOT NULL CHECK (op IN ('create','update','delete')),
  \"values\" TEXT,
  PRIMARY KEY (ordinal, position)
);
CREATE INDEX IF NOT EXISTS ahead_mutation_operation_record ON ahead_mutation_operation (model, identity, ordinal, position);
CREATE TABLE IF NOT EXISTS ahead_mutation_dependency (
  ordinal INTEGER NOT NULL REFERENCES ahead_mutation(ordinal) ON DELETE CASCADE,
  depends_on INTEGER NOT NULL REFERENCES ahead_mutation(ordinal) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('lifecycle','sequence')),
  PRIMARY KEY (ordinal, depends_on),
  CHECK (depends_on < ordinal)
);
CREATE TABLE IF NOT EXISTS ahead_mutation_prerequisite (
  ordinal INTEGER NOT NULL REFERENCES ahead_mutation(ordinal) ON DELETE CASCADE,
  key TEXT NOT NULL, error TEXT,
  PRIMARY KEY (ordinal, key)
);
CREATE TABLE IF NOT EXISTS ahead_rejection (
  ordinal INTEGER PRIMARY KEY, name TEXT NOT NULL, code TEXT NOT NULL, detail TEXT
);
";

/// Refuse a database laid out by an earlier runtime before anything is
/// written to it. Reads only: a refused database is left exactly as found,
/// pending work included ([Reconciliation](../../../docs/engineering/architecture/client/storage/reconciliation.md)).
pub fn check_layout<S: ClientStore>(store: &mut S) -> Result<()> {
    let tables = store.query_committed(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'ahead\\_%' ESCAPE '\\'",
        &[],
    )?;
    let names: Vec<String> = tables
        .rows
        .iter()
        .filter_map(|r| r[0].as_str().map(str::to_owned))
        .collect();
    let refuse = |what: &str| {
        Err(invalid(format!(
            "this database was created by an earlier Ahead runtime ({what}); it cannot be opened by this one. Open a fresh database; the old file is left untouched"
        )))
    };
    for table in LEGACY_TABLES {
        if names.iter().any(|n| n == table) {
            return refuse(&format!("table {table}"));
        }
    }
    for (table, required) in [
        ("ahead_client", CLIENT_COLUMNS),
        ("ahead_mutation", MUTATION_COLUMNS),
    ] {
        if !names.iter().any(|n| n == table) {
            continue;
        }
        let columns = store.query_committed(&format!("PRAGMA table_info({table})"), &[])?;
        for column in required {
            if !columns.rows.iter().any(|r| r[1].as_str() == Some(column)) {
                return refuse(&format!("{table} lacks {column}"));
            }
        }
    }
    Ok(())
}

pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn before_table(model: &str) -> String {
    format!("ahead_before_{model}")
}

pub fn storage_type(value_type: &ValueType) -> &'static str {
    match value_type {
        ValueType::Scalar {
            name: ScalarType::Boolean | ScalarType::Int,
        } => "INTEGER",
        ValueType::Scalar {
            name: ScalarType::Float,
        } => "REAL",
        _ => "TEXT",
    }
}

fn literal(field: &FieldDescriptor) -> Result<String> {
    let value = field.default.as_ref().ok_or_else(|| {
        invalid(format!(
            "column {} is not nullable and has no default",
            field.name
        ))
    })?;
    Ok(match value {
        Value::Bool(b) => i64::from(*b).to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        Value::Null => {
            return Err(invalid(format!(
                "column {} default cannot be null",
                field.name
            )));
        }
        other => format!("'{}'", serde_json::to_string(other)?.replace('\'', "''")),
    })
}

fn column(field: &FieldDescriptor) -> String {
    let null = if field.nullable { "" } else { " NOT NULL" };
    format!(
        "{} {}{null}",
        quote(&field.name),
        storage_type(&field.value_type)
    )
}

fn table_ddl(table: &str, model: &ModelDescriptor) -> String {
    let columns = model
        .fields
        .iter()
        .map(column)
        .collect::<Vec<_>>()
        .join(", ");
    let key = model
        .identity
        .iter()
        .map(|f| quote(f))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE TABLE IF NOT EXISTS {} ({columns}, PRIMARY KEY ({key}))",
        quote(table)
    )
}

pub fn model_ddl(model: &ModelDescriptor) -> Vec<String> {
    let mut statements = vec![
        table_ddl(&model.name, model),
        table_ddl(&before_table(&model.name), model),
    ];
    for fields in &model.unique {
        let name = format!("{}_{}_unique", model.name, fields.join("_"));
        let columns = fields
            .iter()
            .map(|f| quote(f))
            .collect::<Vec<_>>()
            .join(", ");
        statements.push(format!(
            "CREATE UNIQUE INDEX IF NOT EXISTS {} ON {} ({columns})",
            quote(&name),
            quote(&model.name)
        ));
    }
    statements
}

struct Existing {
    columns: BTreeMap<String, String>, // name -> declared type
    identity: Vec<String>,             // pk columns in key order
}

fn existing<S: ClientStore>(store: &mut S, table: &str) -> Result<Option<Existing>> {
    let rows = store.query(&format!("PRAGMA table_info({})", quote(table)), &[])?;
    if rows.rows.is_empty() {
        return Ok(None);
    }
    let mut columns = BTreeMap::new();
    let mut keyed = vec![];
    for row in rows.rows {
        let name = row[1]
            .as_str()
            .ok_or_else(|| invalid("table_info name"))?
            .to_string();
        let ty = row[2].as_str().unwrap_or("").to_ascii_uppercase();
        let pk = row[5].as_i64().unwrap_or(0);
        if pk > 0 {
            keyed.push((pk, name.clone()));
        }
        columns.insert(name, ty);
    }
    keyed.sort();
    Ok(Some(Existing {
        columns,
        identity: keyed.into_iter().map(|(_, n)| n).collect(),
    }))
}

pub fn reconcile<S: ClientStore>(store: &mut S, schema: &Schema) -> Result<()> {
    for model in &schema.models {
        let Some(current) = existing(store, &model.name)? else {
            for statement in model_ddl(model) {
                store.execute(&statement, &[])?;
            }
            continue;
        };
        if current.identity != model.identity {
            return Err(invalid(format!(
                "identity columns of {} changed; cannot open",
                model.name
            )));
        }
        for field in &model.fields {
            match current.columns.get(&field.name) {
                Some(ty) if ty == storage_type(&field.value_type) => {}
                Some(ty) => {
                    return Err(invalid(format!(
                        "column {}.{} is {ty} in the database but {} in the schema",
                        model.name,
                        field.name,
                        storage_type(&field.value_type)
                    )));
                }
                None => {
                    let mut definition = column(field);
                    if !field.nullable {
                        definition.push_str(&format!(" DEFAULT {}", literal(field)?));
                    }
                    for table in [model.name.clone(), before_table(&model.name)] {
                        store.execute(
                            &format!("ALTER TABLE {} ADD COLUMN {definition}", quote(&table)),
                            &[],
                        )?;
                    }
                }
            }
        }
        for statement in model_ddl(model).into_iter().skip(2) {
            store.execute(&statement, &[])?;
        }
    }
    Ok(())
}
