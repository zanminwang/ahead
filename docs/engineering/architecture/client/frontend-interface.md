# Frontend interface

## 1. Introduction and Goals

The frontend interface is the one Rust surface every language binding drives: open a database, run transactions, read, queue, sync, observe. It holds almost no state in memory: the open store, the schema, the client id and a generation counter, plus the registered watchers, the open session if any, and the bounded record of issued pulls and subscription epochs that [Pull](engine/pull.md#5-building-block-view) uses to recognize a page from an earlier subscription. None of that is durable, so a client can be dropped and reopened at any commit.

## 3. Context and Scope

Callers are the [bindings](../sdks/bindings.md), the simulation and the crate tests. Dependencies are a store ([Storage](storage/README.md)), the compiled `schema.json` and the [engine](engine/README.md).

The surface, grouped by purpose:

| Purpose | Operations |
| --- | --- |
| Lifecycle | `open(store, schema)` |
| Writes | `transaction(\|tx\| …)` with `enqueue`, `direct`, `set_channel`, nested `savepoint` and reads inside |
| Session API for hosts that hold a transaction open across calls | `begin_session`, `session(\|tx\| …)`, `session_savepoint`, `session_release`, `session_rollback_savepoint`, `commit_session`, `rollback_session` |
| Reads on the last commit | `read`, `query`, `query_spec`, `related`, `referencing`, `read_sql` |
| Sync | `freeze`, `acknowledge` (returns an `ApplyReport` for the receipt's authority; [Settlement](engine/settlement.md)), `downlink_request` (one pull for every subscribed channel), `apply_page` and `receive_downlink` (return an `ApplyReport` whose `reports` list what could not be applied: read failures, skipped changes, conflicts, divergences; [Pull](engine/pull.md)), plus the `SyncCycle` and `ConnectionDriver` state machines |
| State and control | `pending_count`, `cursor`, `subscriptions`, `subscription_generation` (how many subscribes and unsubscribes committed since open; the [live session](connection/controller/live-session.md) restarts when it changes), `rejections`, `record_status` (pending entries carry `diverged` when their replay failed over new authority), `pending_tasks`, `next_task`, `outcome`, `set_readiness`, `drop_mutation`, `dismiss_rejection` |
| Notification | `watch(tables)` → a receiver signalled when a commit touched one of the tables |

## 5. Building Block View

The interface is the `Client` and `ClientTransaction` types in [client/lib.rs](../../../../crates/client/src/lib.rs); each engine call receives an `Engine` handle ([client/engine.rs](../../../../crates/client/src/engine.rs)) bound to the current transaction.

## 6. Runtime View

**Opening** first checks the layout without writing anything: a database laid out by the checkpoint-era runtime (an `ahead_push_checkpoint` or `ahead_claim` table, or an `ahead_client` row without the completion columns) is refused with "this database was created by an earlier Ahead runtime … Open a fresh database; the old file is left untouched" ([Reconciliation](storage/reconciliation.md)). It then runs the framework DDL and, in one transaction, reconciles the model tables and creates or reads the client row (client id, ordinal and push counters, generation, the last completed push and the frozen declaration). Nothing settles on open: a frozen batch waits for its receipt, and a completed one already has its rows. Opening does not change the generation, so two fresh handles are both valid until one of them writes.

**Every write** goes through one path: begin, run the body, bump the generation with `UPDATE … WHERE generation = ?`, commit. A handle whose generation is behind the database's fails that update with `stale client writer` and rolls back; this is how a forgotten handle is fenced out after another one wrote (guarantee R4). A failed commit is followed by a rollback so the store never stays inside an open transaction.

**Sessions** exist for hosts whose transaction spans several native calls. While a session is open, sync commands are refused, and `commit_session` refuses to commit with an unclosed savepoint.

**Reads outside a transaction** use the committed reader connection, so a long session in the same process does not block them and they do not see its uncommitted writes.

## 10. Quality Requirements

- **A committed write survives close and reopen; identity, queue and rejections persist** (guarantee L2). Evidence: [sqlite/tests/client.rs](../../../../crates/sqlite/tests/client.rs) `open_creates_tables_persists_identity_and_survives_reopen`.
- **An error anywhere in a transaction rolls back the whole transaction; a savepoint confines its own scope** (guarantee L3). Evidence: `local_transaction_and_mutation_savepoint_have_independent_fate`, `session_reads_own_writes_without_notifying_until_commit_and_blocks_other_writes`.
- **A stale handle cannot commit** (guarantee R4). Evidence: `stale_writer_cannot_overwrite_committed_database`.
- **Watchers fire only for the tables they named, and only after commit.** Evidence: `watch_fires_only_for_declared_tables`.
- **A checkpoint-era database is refused on open and left untouched.** Evidence: [sqlite/tests/ddl.rs](../../../../crates/sqlite/tests/ddl.rs) `a_database_from_the_checkpoint_era_is_refused_untouched`.

Executed 2026-09-15: `cargo test -p ahead-sqlite --locked` passed with the tests above.

## 11. Risks and Technical Debt

**Accepted limitation.** `drop_mutation` refuses a mutation that has been frozen, because its outcome is unknown until the receipt arrives. The consequence for a batch the server keeps failing is recorded under [Batching](engine/push/batching.md).

**To confirm.** Applications cannot run code inside the page transaction; [#17](https://github.com/zanminwang/ahead/issues/17) proposes such a hook and notes the binding constraint.
