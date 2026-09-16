# Controller

## 1. Introduction and Goals

The server controller holds one WebSocket per client, learns its channels once, and sends a new page for a channel as soon as a transaction that touched it commits. It reuses the pull engine for every page, so streamed and fetched pages are identical.

## 3. Context and Scope

Input: an authenticated socket from the [transport](transport.md), commit wakes from [Publish](../engine/publish.md). Output: the acknowledgement and pages defined in [Protocol / Subscriptions](../../protocol/subscriptions.md). Negotiation and every page run inside a database transaction through the [backend interface](../backend-interface.md).

## 5. Building Block View

The state is the Rust `Subscriptions`, one per socket: per accepted channel a `ScopeState` with the cursor streamed so far (starting at the head read during negotiation), a *pending* flag set by commits that arrive while a pull is outstanding, and a *running* flag so at most one pull per channel is outstanding; plus one *closed* flag and the client's declared read contracts (`models`), which every `pull` action carries. It is driven by events and answers with an ordered list of actions. The Rust side also decodes the subscribe frame, normalizes scopes, reads heads, and validates each page's scope and cursor before it is sent.

The TypeScript executor (`serveLive`) keeps no sync decision: it carries events in and performs actions out. The Node binding keeps the open sessions in a process-wide registry under a numeric handle, so the controller's state survives between native calls ([Bindings §3](../../sdks/bindings.md#3-context-and-scope)).

Code: `Subscriptions`, `LiveEvent`, `LiveAction`, `decode_subscribe`, `negotiate`, `pull`, `page_progress` in [server/live.rs](../../../../../crates/server/src/live.rs); `negotiateLive`, `liveEvent`, `liveClose` in [bindings/node/src/server.rs](../../../../../bindings/node/src/server.rs); the executor `serveLive` in [server/index.mts](../../../../../packages/server/index.mts).

## 6. Runtime View

1. The executor waits for the first frame. A second client frame at any time closes the socket with `1002`.
2. `negotiateLive` decodes the frame, checks the declared model versions against the config, reads heads and builds the acknowledgement inside one transaction, then opens the session: `Subscriptions::open` answers `listen` for every channel, `send` the acknowledgement, and `pull` every channel from its head, in that order. Listeners are therefore registered before anything is sent, and a commit that lands between the negotiation transaction and the registration is caught by that first pull.
3. The executor performs each action and reports what it learns as an event; the controller answers with the next actions.

| Event | Controller rule | Actions |
| --- | --- | --- |
| `committed {scope}` (a transaction touching the channel committed) | Set *pending*. If no pull is outstanding: clear *pending*, set *running*. | `pull {scope, fromCursor, models}` when a pull starts, otherwise none (the outstanding pull observes *pending* when it returns). |
| `pulled {scope, page}` (the page `pullLive` returned) | Validate the page against the channel and the streamed cursor; advance the cursor. If the page was full, keep pulling; else if *pending*, clear it and pull again; else clear *running*. | `send {frame}` only if the page advanced, then `pull` when the drain continues. |
| `closed` (the socket closed or failed) | Set *closed*, clear every *pending*. Later events produce nothing; a late `pulled` only clears *running*. | none |

| Action | Executor does |
| --- | --- |
| `listen {scope}` | Register a commit listener with the wake hub that dispatches `committed`; remove it on close. |
| `send {frame}` | Send the frame if the socket is open. |
| `pull {scope, fromCursor, models}` | Run `pullLive` in a transaction, at the declared versions, and dispatch `pulled` with its page. Pulls for different channels run concurrently. |

4. A refused declaration (`model_version_unsupported`) closes the handshake with `1002` like a malformed subscribe, without reporting a server failure. An invalid page (`live.invalid_page`) or an event the session cannot accept (`live.invalid_event`: an unknown channel, a `pulled` no pull was asked for, or a handle that is not open) is a defect: the executor reports it and closes with `1011`, as it does when a pull fails; the client reconnects with backoff. On close the executor dispatches `closed`, removes every listener and releases the handle with `liveClose`.

## 10. Quality Requirements

- **Nothing is sent before commit; a rolled-back publication sends nothing; a duplicate receipt wakes nobody; pages split at 50 and the next page starts where the previous ended; reconnecting works.** Evidence: [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `live transport negotiates, wakes only after commit, reconnects, and cleans up`.
- **Only one subscribe frame is accepted and scopes are normalized; a page's cursor progression is checked before sending.** Evidence: [server/tests/runtime.rs](../../../../../crates/server/tests/runtime.rs), [server/tests/stamp.rs](../../../../../crates/server/tests/stamp.rs).
- **Listeners are registered before the acknowledgement and every channel is pulled once from its head; a commit observed during a pull produces exactly one more pull, however many commits arrived; a page that did not advance is not sent; a full page continues from its end and a short page ends the drain; after close no event produces an action and a late page is not sent; an invalid page or an unknown channel is an error.** Evidence: [server/tests/live.rs](../../../../../crates/server/tests/live.rs) (pure transitions, no host).
- **A publication committed between the negotiation transaction and the acknowledgement arrives as a page on the same socket.** Evidence: [runtime.test.mjs](../../../../../integration/persistence/server/runtime.test.mjs) `a publication committed between negotiation and the acknowledgement is delivered by the first drain` (the negotiation result is held on a gate while a push commits; no listener exists yet, and the first drain delivers it after the acknowledgement).

Verified 2026-09-15: `cargo test -p ahead-server --locked` and `bash integration/persistence/server/run.sh` passed after moving the drain policy into `Subscriptions`.

## 11. Risks and Technical Debt

**Potential risk.** Every commit that touches a channel triggers one pull transaction per subscribed socket; there is no shared page cache. Not measured ([#12](https://github.com/zanminwang/ahead/issues/12)).

**Accepted limitations.** Wakes are process-local ([Publish](../engine/publish.md)). Changing channels requires a new socket, and a subscribe may name any number of channels.
