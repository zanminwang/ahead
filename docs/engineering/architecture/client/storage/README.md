# Storage

Storage gives the engine a SQL executor whose tables are the schema record. It contains no sync logic.

- [Store](store.md) — The SQL contract and its SQLite implementation: transactions, savepoints, reads and writes.
- [Reconciliation](reconciliation.md) — Table layout per model and how an existing database is brought in line with a newer compiled schema, or rebuilt beside.

## Code map

| Part | Code location |
|---|---|
| Store | [client/store.rs](../../../../../crates/client/src/store.rs), [sqlite/lib.rs](../../../../../crates/sqlite/src/lib.rs) |
| Reconciliation | [client/ddl.rs](../../../../../crates/client/src/ddl.rs), [client/schema_store.rs](../../../../../crates/client/src/schema_store.rs), `open_at` and `rebuild` in [client/lib.rs](../../../../../crates/client/src/lib.rs), `Schema::compatibility` in [core/schema.rs](../../../../../crates/core/src/schema.rs) |
