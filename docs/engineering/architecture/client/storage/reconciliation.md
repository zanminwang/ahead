# Reconciliation

## 1. Introduction and Goals

Currently, the client stores no schema descriptor. The tables themselves are the record of what schema created them, and opening the client compares them with the compiled schema it was given. Reconciliation makes the tables match when it can do so without losing data, and refuses to open when it cannot.

## 3. Context and Scope

Two steps at open ([Frontend interface](../frontend-interface.md)). The **layout gate** runs first, on the committed reader and before any DDL: it refuses a file laid out by the checkpoint-era runtime and touches nothing. **Reconciliation** then runs once inside the opening transaction, after the framework tables exist and before the client row is read. Input: the names of the `ahead_` tables and the columns of `ahead_client`; then the compiled schema and `PRAGMA table_info` of each model table. Output: the tables match, or an error and an untouched file.

## 5. Building Block View

Per model there are two tables with identical columns: the visible table named after the model and `ahead_before_<Model>` for before images ([Writes](../engine/local-operations/writes.md)). Column types follow [Types](../../schema/types.md); the identity is the primary key in `@@id` order; each `@@unique` becomes a unique index on the visible table. Eight framework tables (`ahead_client`, `ahead_record`, `ahead_subscription`, the four queue tables, `ahead_rejection`) are created with `IF NOT EXISTS`; `ahead_client` carries `last_completed_push` and `push_models` ([Settlement](../engine/settlement.md)).

Code: [client/ddl.rs](../../../../../crates/client/src/ddl.rs) (`check_layout`, `FRAMEWORK_DDL`, `reconcile`).

## 6. Runtime View

What reconciliation does depends on the kind of difference. The three outcomes are different in kind and should not be summarized as "schema changes are refused":

| Difference between schema and table | Outcome |
| --- | --- |
| An earlier framework layout: an `ahead_push_checkpoint` or `ahead_claim` table, `ahead_client` without `last_completed_push` and `push_models`, or `ahead_mutation` without `diverged` | **refused** before any DDL, with "this database was created by an earlier Ahead runtime (…); it cannot be opened by this one. Open a fresh database; the old file is left untouched"; every row, checkpoint and pending operation included, stays as found |
| Model table missing | created, with its before table and indexes |
| Field missing from the table, nullable | column added to both tables |
| Field missing, non-nullable, default in the descriptor | column added with that default |
| Field missing, non-nullable, no default | **refused**; open fails |
| Identity columns differ | **refused** |
| Column storage type differs | **refused** |
| Column in the table but not in the schema | **kept**, never read or written |
| Unique index no longer declared | **kept**, still enforced |
| Enum value set changed | **not detected**; the column is `TEXT` |
| Model removed from the schema | its tables are **kept** |

Consequences worth knowing: a field rename is handled as "remove the old field, add the new one", so the old column stays in place and the new column follows the rows above for an added field: filled with `null` if nullable, with the declared default if it has one, and refused (open fails) if it is non-nullable without a default; the old column's values are not carried over. A stale unique index keeps constraining rows; and rows holding an enum value the schema no longer declares remain readable as strings that normalization will reject. A refused reconciliation rolls back and leaves the file exactly as it was; the only remedy today is a new database file. The layout gate is deliberately the same answer: an old file's frozen batches referred to a receipt shape and a settlement rule this runtime no longer has, so it neither migrates nor deletes them ([Protocol / Push §11](../../protocol/push.md#11-risks-and-technical-debt)).

## 9. Architecture Decisions

**Detect compatibility and rebuild incompatible replicas — agreed, not implemented ([#20](https://github.com/zanminwang/ahead/issues/20)).** Generated clients carry the current schema. Store the schema descriptor when creating a local database; on each open, Rust compares that stored descriptor with the incoming schema using the [agreed compatibility rules](../../schema/models.md#9-architecture-decisions). A version or hash can identify a change, but cannot classify compatibility on its own.

| Comparison | Target behavior |
| --- | --- |
| Unchanged | Open the existing database. |
| Compatible change, such as a nullable-field addition | Apply the supported additive change and update the stored descriptor atomically. |
| Incompatible | Create a database matching the new schema and synchronize server data from the beginning; do not reuse the old delivery cursors as evidence that the new database is populated. |

This is automatic framework behavior, without application migration SQL, migration commands or manually registered upgrade callbacks. Reuse a compatible database rather than rebuilding on every startup. Retain the old database; selecting and switching databases and recovering interrupted rebuilds need implementation design.

Continuing unfinished mutations and preserving access to local-only records across incompatible schemas are deferred follow-up work. Retaining the old file does not itself make those operations available in the new database. Do not delete the old database or rewrite frozen requests, and do not claim seamless upgrades for these cases until they are handled. Existing databases without a stored descriptor also need an explicit handling rule.

## 10. Quality Requirements

- **Additive changes open and fill existing rows; unknown columns survive.** Evidence: [sqlite/tests/ddl.rs](../../../../../crates/sqlite/tests/ddl.rs) `adds_missing_columns_to_both_tables_and_keeps_unknown_ones`.
- **Identity, type and default-less non-nullable changes are refused without touching the file**. Evidence: `rejects_non_nullable_column_without_default_identity_change_and_type_change`.
- **A fresh database gets model, before and framework tables and enforces unique indexes.** Evidence: `creates_model_before_and_framework_tables`.
- **A checkpoint-era layout is refused before any write, and the file, its checkpoint rows and its `ahead_client` row are left exactly as found.** Evidence: `a_database_from_the_checkpoint_era_is_refused_untouched` (a checkpoint table beside the current layout, and an `ahead_client` table without the completion column).

Executed 2026-09-15: `cargo test -p ahead-sqlite --locked` passed with the tests above. Reconciliation with a non-empty queue is not tested; the claim that queued operation bytes survive an additive change follows from the row layout.

## 11. Risks and Technical Debt

**Problem: a non-nullable field cannot be added to a model with data.** The descriptor supports a default, but the compiler cannot emit one ([Models](../../schema/models.md)), so the "added with default" row above is unreachable from a `.model` file. Tracked in [#27](https://github.com/zanminwang/ahead/issues/27) and [#20](https://github.com/zanminwang/ahead/issues/20).

**Accepted limitation (contract to be decided in [#20](https://github.com/zanminwang/ahead/issues/20)).** Kept columns, kept indexes and undetected enum changes are the current behavior, not a design; #20 lists the open decisions, with the target comparison/rebuild direction in section 9; constraint compatibility and transition details still need design.
