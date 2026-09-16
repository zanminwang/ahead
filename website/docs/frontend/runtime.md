# Client runtime

The generated client delegates to a generic runtime, available as `client.client`. Most applications use the [generated model APIs](client-api.md); use this reference for SQL, connection control, recovery or a custom integration. TypeScript returns promises and Dart returns futures unless stated otherwise. Native validation failures reject the call; Dart reports them as `StateError`.

## Opening and schema changes

TypeScript imports `Client` from `packages/client-js/index.mts` in a source checkout. Dart imports `package:ahead/ahead.dart` through the local package dependency described in the [client setup guide](setup.md).

=== "TypeScript"

    ```ts
    const raw = await Client.open({ path: 'local.sqlite', schema });
    ```

=== "Flutter"

    ```dart
    final raw = await Client.open(
      path: 'local.sqlite', schema: schema,
      libraryPath: '/absolute/path/to/libahead_dart.dylib',
    );
    ```

`schema` is the compiler's generated descriptor. `clientId` is a read-only, persistent identity for that database, used for retry deduplication. Use one active client per database and a separate file per signed-in user. Do not duplicate a database and then let both copies independently send mutations under the same client identity.

Both forms accept `migration`. For an explicitly changed descriptor:

=== "TypeScript"

    ```ts
    const raw = await Client.open({
      path: 'local.sqlite',
      schema,
      migration: { defaults: { Entry: { addedField: null } }, replayPull: true },
    });
    ```

=== "Flutter"

    ```dart
    final raw = await Client.open(
      path: 'local.sqlite', schema: schema,
      libraryPath: '/absolute/path/to/libahead_dart.dylib',
      migration: {
        'defaults': {'Entry': {'addedField': null}},
        'replayPull': true,
      },
    );
    ```

Defaults must fit the new field's type. Migration is atomic and preserves client identity, queued work and frozen request bytes. `replayPull` requests cursor rewind when applying the changed descriptor. Arbitrary identity/type changes are not supported; see [local storage](storage.md).

## Reads

All these methods read local SQLite through Rust. `RecordValue` in TypeScript is `Record<string, unknown>`; Dart uses `Map<String, dynamic>`.

| Method | Input | Result |
| --- | --- | --- |
| `read(model, identity)` | Model name and all identity fields | Complete record or null |
| `query(model, where)` | Equality fields; default empty | Matching records |
| `querySpec(model, query)` | `filter`, `orderBy`, `limit` | Matching records with requested order/limit |
| `readSql(sql, parameters)` | Read-only SQL and bound parameters | Result rows |
| `related(model, identity, relation)` | Source identity and declared relation name | Related record or null |
| `referencing(model, identity, source, relation)` | Target identity, referencing model and its relation | Referencing records |
| `watch(model, where, listener, onError?)` (TypeScript) | Equality filter and callbacks | Unsubscribe function |
| `watch(model, where: ...)` (Dart) | Equality filter | Stream of record lists |

TypeScript's `query` and `readSql` take optional positional second arguments. Dart uses named `where:` and `parameters:`. `querySpec` takes the same positional descriptor in both languages:

=== "TypeScript"

    ```ts
    const rows = await raw.querySpec('Entry', {
      filter: { note: null },
      orderBy: [{ field: 'text', direction: 'ascending' }],
      limit: 20,
    });
    const matches = await raw.readSql(
      'SELECT id, text FROM "Entry" WHERE text = ?', ['Draft'],
    );
    ```

=== "Flutter"

    ```dart
    final rows = await raw.querySpec('Entry', {
      'filter': {'note': null},
      'orderBy': [{'field': 'text', 'direction': 'ascending'}],
      'limit': 20,
    });
    final matches = await raw.readSql(
      'SELECT id, text FROM "Entry" WHERE text = ?',
      parameters: ['Draft'],
    );
    ```

`querySpec` calls its equality filter `filter`; the generated API calls it `where`. SQL rejects writes. Bind values instead of interpolating them into SQL. Watch results are distinct committed snapshots, with an initial query; they are not an event log. Cancel watchers when their owner is disposed.

## Transactions and savepoints

`transaction<T>(callback)` commits the callback's result or rolls back on failure. Its `Transaction` exposes all the reads above except `watch`, plus:

| Method | Behavior |
| --- | --- |
| `mutate(descriptor)` | Apply declared optimistic operations and enqueue one mutation; returns its local ordinal |
| `direct(operation)` | Apply one local-only operation; returns void |
| `savepoint(callback)` | Run a nested scope; roll back that scope on failure; return its callback result |

`raw.mutate(descriptor)` is a convenience wrapper around its own transaction. Prefer generated mutation builders to constructing descriptors yourself.

=== "TypeScript"

    ```ts
    import { Edit } from './generated/client.ts';

    await raw.transaction(async tx => {
      await tx.mutate(Edit({
        entry: { identity: { id: 'entry-1' }, values: { text: 'Draft' } },
      }));
      try {
        await tx.savepoint(async () => {
          await tx.direct({
            model: 'Entry', op: 'update',
            identity: { id: 'entry-1' }, values: { note: 'Temporary' },
          });
          throw new Error('Discard this note');
        });
      } catch {
        // The note rolls back; the earlier edit can still commit.
      }
    });
    ```

=== "Flutter"

    ```dart
    import 'generated/generated.dart';

    await raw.transaction((tx) async {
      await tx.mutate(edit(
        entry: const EditEntryUpdate(
          identity: EntryIdentity(id: 'entry-1'), text: Present('Draft'),
        ),
      ));
      try {
        await tx.savepoint(() async {
          await tx.direct({
            'model': 'Entry', 'op': 'update',
            'identity': {'id': 'entry-1'}, 'values': {'note': 'Temporary'},
          });
          throw StateError('Discard this note');
        });
      } catch (_) {
        // The note rolls back; the earlier edit can still commit.
      }
    });
    ```

Dart's savepoint also takes a zero-argument async callback and operates through the same `tx`. Await every call and nested callback. Savepoints must be properly nested, not run concurrently. An escaped transaction, unfinished operation or overlapping savepoint fails. Inside the transaction use `tx` reads; an outer `raw` read can wait behind the current transaction. TypeScript's `Transaction.finish()` is runtime-owned bookkeeping; applications should not call it.

## Server connection

Pass `server` when opening the generated client, or call `connect` on the raw client after opening local storage. TypeScript accepts `ServerOptions`; Dart uses `SyncServer`. Only one connection may be active per client. Network I/O happens outside the local transaction queue.

=== "TypeScript"

    ```ts
    const connection = await raw.connect(
      { url: backendUrl, token: () => accessToken },
      {
        onError: error => console.error(error),
        refreshAuth: async () => { accessToken = await renewAccessToken(); },
      },
    );
    ```

=== "Flutter"

    ```dart
    final connection = await raw.connect(
      SyncServer(url: backendUrl, token: () => accessToken),
      onError: (error) => print(error),
      refreshAuth: () async { accessToken = await renewAccessToken(); },
    );
    ```

| Option | TypeScript | Dart |
| --- | --- | --- |
| `url` | HTTP or HTTPS backend base URL | HTTP or HTTPS backend base URL |
| `token` | String or function returning a string/promise | Function returning a string/future |
| `onError` | `(error: unknown) => void`, in connection options | Named callback on `connect` / `open` |
| `refreshAuth` | `() => Promise<void>`, in connection options | Named async callback on `connect` / `open` |

Here `backendUrl`, `accessToken` and `renewAccessToken` belong to your application. Credentials travel in authorization headers. Token functions run for new requests and connections, so they can read refreshed credentials. Authentication failures can invoke `refreshAuth`; background failures reach `onError` and retry with backoff.

### Catch-up and live updates

Ahead manages these phases automatically:

1. Connect to `/sync/live` and subscribe to the current channel set. The server installs listeners, then acknowledges the subscription with each channel's current position.
2. If a saved cursor is behind, fetch missing records through one `POST /sync/pull` for all channels, repeated while a channel has more. Queue WebSocket pages arriving while catch-up runs. If every cursor is current, skip this step.
3. Continue receiving WebSocket updates. HTTP and WebSocket pages enter the same serialized Rust processing path, using each channel's saved cursor.

For either source, a page applies as one transaction and names a range for each channel it covers. A channel already covered by its cursor is left alone. A range spanning the current cursor applies: for example, at cursor `100`, a range `90 → 120` advances the channel to `120`, and each record's stamp decides whether its content is newer. A range starting beyond the current cursor is a gap. Then nothing from the page applies, and HTTP recovery fetches the missing range. Pages update SQLite and watches through the same engine logic.

Mutation submission runs independently through `POST /sync/mutations`. A connection with no subscribed channels can still submit mutations without opening a socket.

Reconnection and subscription changes repeat catch-up from saved progress. The client checks that every HTTP response and queued WebSocket page belongs to the current session before applying it. Pause and close cancel requests and sockets; resume creates a new session. The runtime does not poll for remote changes.

## Connection controls

TypeScript calls the returned object `Connection`; Dart calls it `RuntimeConnection`.

| Method | Behavior |
| --- | --- |
| `pause()` | Stop background network work; local reads/writes remain available |
| `resume()` | Resume a paused connection and schedule work |
| `wake()` | Ask the driver to re-evaluate pending work |
| `close()` | Permanently stop this connection; the client database stays open |
| `closed` (Dart) | Future that completes when the connection closes |

All controls return promise/future void. Pause/close cancel network activity and discard responses from the canceled session. Persisted frozen requests remain available for retry. After close, create a new connection through the raw client to resume sync. `await raw.close()` closes its connection and native database resources and is idempotent; subsequent client operations fail.

## Pending work and recovery

`status()` returns `{ clientId, pending, beforeImages, cursors, channels, rejections }`. `pending` counts queued mutations; `beforeImages` is a diagnostic count; `cursors` maps channels to received positions; `channels` lists desired subscriptions; `rejections` contains `{ ordinal, code }` entries. It is a local snapshot, not a network status probe.

=== "TypeScript"

    ```ts
    const status = await raw.recordStatus('Entry', { id: 'entry-1' });
    for (const item of status.pending) console.log(item.ordinal, item.phase);
    for (const rejection of (await raw.status()).rejections) {
      console.log(rejection.code);
      // After your UI has handled it:
      await raw.dismissRejection(rejection.ordinal);
    }
    ```

=== "Flutter"

    ```dart
    final status = await raw.recordStatus('Entry', {'id': 'entry-1'});
    for (final item in status['pending'] as List) {
      print('${item['ordinal']}: ${item['phase']}');
    }
    for (final rejection in (await raw.status())['rejections'] as List) {
      print(rejection['code']);
      // After your UI has handled it:
      await raw.dismissRejection(rejection['ordinal'] as int);
    }
    ```

| Method | Result / effect |
| --- | --- |
| `recordStatus(model, identity)` | `{ pending, rejections }` for that record; pending entries include ordinal, mutation name, phase, prerequisite states and `diverged` |
| `dismissRejection(ordinal)` | Remove a handled rejection from the durable local inbox; does not retry it |
| `drop(ordinal)` | Remove eligible unsent work and recompute local state; frozen/sent work cannot be cancelled this way |

Phases are `queued` (not frozen) and `frozen` (request retained for sending or retry); a receipt completes a frozen mutation and removes it, so there is no phase after `frozen`. A mutation ordinal is local bookkeeping. To retry a rejected business operation, make a new edit after resolving the cause. See [sync and recovery](sync.md).

## Prerequisites

A schema can require host I/O, such as an upload, before a mutation can be sent. The local change remains visible while this work is pending.

=== "TypeScript"

    ```ts
    await raw.runPrerequisites({
      Uploaded: async args => { await uploadFile(args.key); },
    });
    ```

=== "Flutter"

    ```dart
    await raw.runPrerequisites({
      'Uploaded': (args) async { await uploadFile(args['key']); },
    });
    ```

`uploadFile` is application code. Dart accepts the equivalent map of async callbacks. Callbacks run one at a time and must tolerate retry after a crash or restart; starting a connection does not automatically supply or run your host callbacks.

| Method | Behavior |
| --- | --- |
| `pendingTasks()` | Return unresolved tasks, including `key`, `state`, schema-derived `name`/`arguments` and, for a failed task, `error` |
| `runPrerequisites(handlers)` | Run pending tasks; success marks ready, a callback failure marks failed with the error's text, a task with no handler is marked failed with `missing prerequisite handler` |
| `setReadiness(key, state)` | Set `ready`, `pending` or `failed`; use the task's opaque key, not a reconstructed key |

Callback failures are recorded as failed tasks with their reason rather than rethrown by the runner; a task no handler covers is recorded the same way and the run goes on. Inspect `pendingTasks` or `recordStatus` to display them. To retry, set the failed key to `pending`, then run callbacks again. Mark ready only when the prerequisite actually completed.

## Protocol primitives

These low-level engine methods support protocol tests and tooling. Application synchronization is managed by `connect`; calling these methods alongside an active connection can interfere with its sequencing.

| Method | Input and return |
| --- | --- |
| `freeze()` | Return frozen request JSON or null when no batch can be sent; retry preserves the same bytes |
| `acknowledge(sequence, receipt)` | Apply a decoded push receipt to the matching batch; returns what it applied and any reports |
| `applyPull(page)` | Apply a decoded pull page as one transaction and return its application result, including reports for records it could not apply |

Wire fields are defined in the [protocol source](https://github.com/zanminwang/ahead/blob/main/crates/core/src/protocol.rs) and exercised by [shared wire fixtures](https://github.com/zanminwang/ahead/blob/main/fixtures). Do not manufacture receipts, advance cursors yourself or rewrite frozen requests to recover from a network failure.
