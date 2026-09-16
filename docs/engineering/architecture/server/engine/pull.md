# Pull

## 1. Introduction and Goals

Server pull answers "what changed in this channel after position X" with whole records, loaded by the application under its own visibility rules, in a page the client can apply in order.

## 3. Context and Scope

Input: the owner, a [pull request](../../protocol/pull.md) (or, for the live stream, a channel and cursor with client id `live`) and a host. Output: one page, or an error that aborts the request (cursor ahead of head, malformed invalidation rows, unregistered model, non-canonical identity, missing stamp, misaligned or malformed loader result, or a loader refusal, `loader.refused`). Both the HTTP route and the live drain call it ([Server / Connection / Controller](../connection/controller.md)).

## 5. Building Block View

Pull reads two things: the **invalidation table**, which holds one row per `(channel, model, record)` at the record's latest position in that channel, joined with the record's *current* stamp from the stamp table (`scan`; a row whose record has no stamp metadata is a storage error), and the **loaders**, which supply current content. It writes nothing. The stamp comes from the record, not the invalidation row, because a change advances a stamp whether or not it is published ([Publish](publish.md)); the cursor is delivery progress only.

Code: `process_pull` in [server/lib.rs](../../../../../crates/server/src/lib.rs); the live wrapper in [server/live.rs](../../../../../crates/server/src/live.rs).

## 6. Runtime View

1. Read the channel head; a request beyond it is refused, so a client cannot skip ahead.
2. Scan up to `limits::PULL_CHANGES` (50) invalidation rows after the client's cursor, in cursor order, and check each: same channel, strictly increasing, within the head, a registered model, a canonical identity key, a positive stamp.
3. Group the rows by model and serve each at the version the request declared (`models`, checked before the scan: every declared model known, every version retained, else `model_version_unsupported` with the model and version). Call that version's loader once with all identities for the model (`load {model, version, identities, owner}`; no channel) and normalize each returned row against the retained contract of that version, not the current schema (identity may be included, nullable fields may be omitted); `null` becomes a delete. A loader that answers a refusal code instead of rows fails the page with `loader.refused` naming the model and code; nothing is skipped and the cursor does not move. A row whose model the client did not declare is not in its read contract: the pull is refused whole with `model_version_unsupported` naming the model, never skipped or served at a guessed version, until per-read isolation ([#95](https://github.com/zanminwang/ahead/issues/95)).
4. Set the page end: the last row's cursor if the scan was full, otherwise the head, so a client does not stall behind positions that compaction emptied.

**Compaction.** Because a record has one row per channel, publishing it again moves that row to a new cursor and leaves a hole at the old one. A client that already applied the old position sees the record again later with a newer stamp; the stamp makes that harmless.

**Coherence.** Head, scan and load must observe one snapshot. That is a requirement on the application's transaction runner ([Persistence](../persistence.md)); the shipped Prisma runner uses repeatable read.

## 9. Architecture Decisions

**Loader failure isolation — agreed target ([D7](../../../guarantees.md#d-distribution), [#95](https://github.com/zanminwang/ahead/issues/95)).** A failure attributable to a read, including an unsupported model version, is a read error: the loader may throw, and the runtime reports the error to the application. Keep mutation rejection records, but do not introduce a durable loader-failure queue. Unrelated reads continue, including those sharing the same page or channel. An entire channel must not be paused solely because one of its reads failed. Preserve existing local data; a failed load is not a `null` deletion or successful synchronization.

The current implementation groups identities by model and propagates loader errors and refusals as request errors (`500 server` for a thrown loader, `loader.refused` for a refusal). A failed read can be requested again; it does not reject a previously accepted mutation. Background errors must reach the application through an error event or status rather than an unhandled exception. Error reporting and isolation within a multi-record loader call remain to be designed. Failed reads must not advance the cursor or count as delivered authority; infrastructure failures may require retrying the request. Inside a push the same refusal is that mutation's rejection ([Push §9](push.md#9-architecture-decisions)).

**Loaders name no channel ([#55](https://github.com/zanminwang/ahead/issues/55)).** `load` carries the model, the version, the identities and the owner. The record a page delivers is therefore the record a receipt delivers at the same stamp (guarantee D4); a channel selects which records are delivered, never alternate contents. A loader that needs to hide a record from a user returns `null` for it or refuses the read.

## 10. Quality Requirements

- **Every change carries the record's current stamp as the scan returned it; rows without a positive stamp are refused** (guarantee D2, server side). Evidence: [server/tests/stamp.rs](../../../../../crates/server/tests/stamp.rs) `pull_copies_the_row_stamp_into_the_change`, `pull_rejects_rows_without_a_positive_stamp`; [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `scan pairs the invalidation cursor with the current record stamp; a missing record row is a storage defect`.
- **Compaction delivers the latest state once; a deletion is an aligned `null`; a full page ends at its last row and the remainder reaches the head.** Evidence: [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `compaction materializes latest state; deletion is aligned null`, `50-row pages retain original cursor progression and remainder reaches head`.
- **Loaders receive no channel; head, scan and loader stay coherent under concurrent publication.** Evidence: `loaders receive no channel`, `repeatable-read runner keeps head, scan, and loader coherent across concurrent publication`.
- **A loader refusal fails the page with `loader.refused` and moves no cursor; a loader defect aborts the pull the same way.** Evidence: `loader defects abort pull instead of silently advancing its cursor`; the push half of `a loader refusal during push rejects only that mutation; so does a thrown loader error` (a thrown loader error is isolated to its mutation in a push, but still fails the page in a pull); the `load` refusal case in [server/tests/host_contract.rs](../../../../../crates/server/tests/host_contract.rs) `a_load_response_is_rows_or_a_refusal_code`.
- **A pull is served at the declared model version: `load` names it, only that version's loader runs, its rows are normalized with that version's contract, and an undeclared model or unretained version is refused with the model named.** Evidence: `a_page_holding_a_model_the_client_did_not_declare_is_refused_whole` and the assertions below; [server/tests/stamp.rs](../../../../../crates/server/tests/stamp.rs) `pull_copies_the_row_stamp_into_the_change`, `pull_normalizes_loader_rows_with_the_retained_contract_of_the_served_version`; [server/tests/runtime.rs](../../../../../crates/server/tests/runtime.rs) `startup_validates_the_retained_model_contracts`; [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `a pull reaches the loader of the declared model version and normalizes rows with that contract`.

Executed 2026-09-15: `cargo test -p ahead-server --locked` passed with the Rust tests above; `bash integration/persistence/server/run.sh` (61 passed) executed the same day.

## 11. Risks and Technical Debt

**Problem: a model the client did not declare aborts the whole pull.** A client older than the server (the server added a model) cannot read that model, so today its pull of any channel holding such a record is refused, and the channel stalls until the client upgrades. Target D7 wants that record reported as one failed read while the rest of the page proceeds; the report shape, cursor handling and the client's reaction are [#95](https://github.com/zanminwang/ahead/issues/95). Evidence: `a_page_holding_a_model_the_client_did_not_declare_is_refused_whole`.

**Problem: a loader failure aborts unrelated reads in the page.** `process_pull` propagates loader errors as request errors without isolating and reporting the affected read, contrary to target D7. No tests establishing D7 have been run for this documentation change. Track the protocol, recovery and cursor design in [#95](https://github.com/zanminwang/ahead/issues/95), alongside malformed-record handling in [#51](https://github.com/zanminwang/ahead/issues/51).

**Accepted limitation (planned changes).** Page size is the protocol's fixed 50 with count-based completion ([#11](https://github.com/zanminwang/ahead/issues/11)); bootstrap is a cursor walk from zero over every model ([#14](https://github.com/zanminwang/ahead/issues/14) proposes snapshots).

**Accepted limitation, worth stating.** The loader is the only visibility control: a loader that ignores `userId` exposes every record it is asked for to any authenticated user, on every channel that delivers it.
