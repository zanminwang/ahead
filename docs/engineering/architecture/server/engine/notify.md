# Notify

## 1. Introduction and Goals

Notify is how a change becomes visible to pull. When application code says "this record changed in this channel", notify gives the record a new position in that channel and a new version number, inside the same transaction as the change, and wakes live subscribers once that transaction commits.

## 3. Context and Scope

Three ways in, one path:

| Caller | Call | When it publishes |
| --- | --- | --- |
| a handler | `notify({channel, records})` | buffered, published after the handler returns |
| application code with a bound transaction | `bindTransaction(tx).notify(…)` | immediately, awaited |
| application code, shortcut | `backend.notify(tx, …)` | immediately, awaited |

Each publication asks [Persistence](../persistence.md) to allocate the next *stamp* for the record and the next *cursor* for the channel and to upsert the invalidation row ([Pull](pull.md)). The set of channels touched in a transaction feeds the wake after commit ([Server / Connection / Controller](../connection/controller.md)).

## 5. Building Block View

- **Stamps and cursors are independent counters.** A stamp is per record and orders content; a cursor is per channel and orders pages. One notify to channels A and B allocates two consecutive stamps (in notify order) and one new cursor in each channel (guarantee D3).
- **Publish order is notify order**, because buffered calls are published sequentially after the handler returns.
- **Wake set.** The session records every channel published in the transaction, snapshots that set at each mutation's savepoint and restores it on rollback, so a rejected mutation's publications neither remain nor wake anyone. After the transaction commits, an in-process hub calls the wake callbacks registered by live sockets for those channels.

Code: `publish` in [server/lib.rs](../../../../../crates/server/src/lib.rs); buffering, `Session.touched` and `WakeHub` in [server/index.mts](../../../../../packages/server/index.mts).

## 6. Runtime View

Inside a push: handler runs → buffered notifies publish → the settlement channel's head becomes the checkpoint → receipt → commit → wakes. Outside a push with `bindTransaction`: `notify` (awaited) → `assertCommittable` → commit → the application calls the function returned by `afterCommit()` to wake subscribers.

## 10. Quality Requirements

- **Each publication allocates its own stamp and channel cursors advance independently; concurrent notifies of one record never share a stamp** (guarantee D3). Evidence: [server/tests/stamp.rs](../../../../../crates/server/tests/stamp.rs) `publish_requires_cursor_and_stamp_from_the_host`; [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `publish allocates one stamp per notify and stores it on the invalidation row`, `concurrent notifies of one record receive distinct stamps`.
- **A rejected mutation publishes nothing, and a rolled-back transaction publishes nothing.** Evidence: `rejected mutation publishes nothing even though it called notify first`, `publication rollback uses user transaction and rejects unregistered models`.
- **Subscribers are woken only after commit, and never by a duplicate receipt.** Evidence: `live transport negotiates, wakes only after commit, reconnects, and cleans up`.

Tests read, not executed.

## 11. Risks and Technical Debt

**Problem: the shortcut `backend.notify(tx, …)` never wakes live subscribers.** *Condition:* application code publishes outside a push without `bindTransaction`. *Consequence:* the publication is stored, but the touched set is discarded, so connected clients learn of the change only when they reconnect and catch up. The round-trip fixture backend and the To-do example seed use this shortcut. *Evidence:* `publish` falls back to a throwaway session in [server/index.mts](../../../../../packages/server/index.mts); [fixtures/round-trip/server.mts](../../../../../integration/e2e/fixtures/round-trip/server.mts). **To confirm:** remove the shortcut or document the `afterCommit` requirement.

**Accepted limitation.** Wakes are in-process: a second server instance, or a publication from another process, does not wake this process's sockets; those clients catch up on reconnect. No issue tracks an external pub/sub.

**Potential risk.** Every publication to a channel updates the same channel row under a row lock, so handlers touching one hot channel serialize and may retry on serialization failure. Not measured ([#12](https://github.com/zanminwang/ahead/issues/12)).
