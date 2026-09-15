# End-to-end tests

Verify complete application paths through a generated client, the real backend, PostgreSQL and local SQLite. Use a small number of representative flows that can detect missing wiring or incompatible assumptions between components.

Entry points: [round-trip.test.mjs](../../../integration/e2e/round-trip.test.mjs) drives the [round-trip fixture](../../../integration/e2e/fixtures/round-trip/README.md) with TypeScript and Dart clients (local visibility, server responses, live reconnect); [todo.test.mjs](../../../integration/e2e/todo.test.mjs) drives the public [To-do example](../../../examples/todo/README.md) backend.

After installing the prerequisites in [Running tests](running.md):

```sh
bash integration/e2e/run.sh
bash integration/e2e/todo-run.sh
```

Each runner builds native artifacts, generates its fixture or example APIs and starts a temporary PostgreSQL cluster. Assert user-visible state through the client, including rejected writes and resumed sync.

Next review: identify which full paths need a release gate and which cases are better isolated in component or integration tests. Device startup is separately exercised by [platform smoke tests](../../../integration/platform/README.md).

## Coverage review

Reviewed 2026-09-14, To-do row added 2026-09-15; tests read, not executed. The suite runs a real backend over a temporary PostgreSQL cluster with the Node client, the Dart client and the fixture CLI.

| Path | Test | What it establishes | Limits |
| --- | --- | --- | --- |
| Node client → HTTP → Rust backend → Prisma → SQLite, then Dart against the same server | [round-trip.test.mjs](../../../integration/e2e/round-trip.test.mjs) first test | initial sync, offline edit visible before sync, frozen batch across restart, lost receipt converges without re-executing, rejection reported, local writes not blocked by an in-flight push, background connection with pause and resume, Dart write settles | The Node part drives the wire through the internal `syncProtocol` fixture for the first half and `connect` for the second; the Dart part writes a different record, so identical outcomes are not compared. |
| Fixture console client | second test | offline edit stays local, syncs when online, normalized value comes back | Depends on the fixture's console output strings. |
| Built-in live sync in both languages | third test with [dart_live_client.dart](../../../integration/e2e/dart_live_client.dart) | multi-page catch-up (56 records), a commit during a held catch-up is not missed, watch fires, dependent pushes settle from streamed pages without polling, offline reconnect resumes from the persisted cursor, Dart repeats the flow including unsubscribe and resubscribe | Timing assertions use polling with fixed timeouts; a slow host can produce false failures rather than false passes. |
| To-do backend scenarios | [todo.test.mjs](../../../integration/e2e/todo.test.mjs) | seeds survive a restart, happy path convergence, 401 for an unknown identity, `todo.missing`, `todo.id_conflict` without overwrite, lost receipt runs the handler once, offline add-then-done across a reopen, opposing completions in both orders | Node clients only; the two-phone flow runs in the [To-do simulator smoke](../../../integration/platform/run_todo_ios_smoke.sh), outside the host gate. |
| iOS device startup | [platform smoke](../../../integration/platform/run_ios_simulator_smoke.sh) | the native library loads and the app starts on a simulator | Manual, outside the host gate. |

The suite is the only place the real TypeScript backend, the real PostgreSQL adapter and a real generated client meet. It should stay small; each of its assertions is also covered at a lower level except the wiring itself and the `backend.notify(tx, …)` shortcut used by the fixture server and the To-do seed, which relies on catch-up rather than a wake ([Notify §11](../architecture/server/engine/notify.md)).
