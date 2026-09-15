# Backend interfaces

Your backend implements the write path through handlers and the read/sync path through loaders. The compiler generates their TypeScript interfaces from your schema. Ahead supplies protocol processing; your application supplies business logic, authorization and a database transaction.

Examples use the `Entry` / `Edit` schema of the [round-trip fixture](https://github.com/zanminwang/ahead/blob/main/integration/e2e/fixtures/round-trip/models/entry.model), which keeps a nullable field and an update patch that the [To-do example](../getting-started.md) does not need. The complete working implementation is [server.mts](https://github.com/zanminwang/ahead/blob/main/integration/e2e/fixtures/round-trip/server.mts); the To-do backend is [examples/todo/server.mts](https://github.com/zanminwang/ahead/blob/main/examples/todo/server.mts).

## createBackend

```ts
import { PrismaClient, type Prisma } from '@prisma/client';
import { createBackend, devAuth } from './generated/backend.ts';
import { prisma } from '../../packages/persistence-prisma/index.mts';
import { handlers } from './handlers.ts';
import { loaders } from './loaders.ts';

const db = new PrismaClient();
const backend = createBackend<Prisma.TransactionClient>({
  database: prisma(db),
  authenticate: devAuth(),
  handlers,
  loaders,
  onError: error => console.error(error),
});
const server = await backend.listen({ port: 4242 });
console.log(server.url);
```

The imports assume the same directory depth as the repository example. `handlers.ts` and `loaders.ts` contain the implementations below. Generate the Prisma application client and apply Ahead's metadata migration first; see [database adapters](database.md).

The generated `Options<Tx>` requires:

| Option | Responsibility |
| --- | --- |
| `database: Database<Tx>` | Run transactions and bind sync persistence to the supplied transaction |
| `authenticate: Authenticate` | Resolve the caller's user identity or reject the request |
| `handlers: Handlers<Tx>` | Implement each supported mutation version |
| `loaders: Loaders<Tx>` | Implement the read function for each model |

Optional options are `translateRejection`, `onError`, `loaderHooks` and `native`, described below. The generated function binds the schema and returns the backend synchronously. The generic function in `packages/server/index.mts` additionally requires `config`; normal generated integrations do not pass it.

## What your backend owns

The Rust runtime processes the sync protocol and nothing else. The rules below are yours to implement; the runtime neither enforces nor checks them, and the schema does not make it do so.

| Rule | Who owns it | What the runtime does |
| --- | --- | --- |
| Authorization | Handlers decide what `userId` may write; loaders decide what `userId` may see on `channel` and return `null` for the rest. | Authenticates the request and passes `userId` and `channel` through. There is no channel-level policy. |
| Unique constraints and identities | Your database schema. `@@unique` and `@@id` are enforced on the client only; the client's local database refuses a violating write, but nothing checks the server. | Decodes identities and patches by shape. A duplicate that your database allows is stored. |
| Child deletion | Your handler. `onTargetDelete: delete` is a client-side cascade: the client deletes the children locally, and those deletes never reach the server. A handler that deletes a parent must delete its children itself and notify each channel that delivered them. | Delivers the parent's delete when the handler notifies it. |
| Client identity | Each signed-in user gets their own local client database. A client id is bound to the first user that pushed with it; a push from another user with the same client id answers `403 client.owner_mismatch`, and there is no reassignment. | Stores the owner with the client row. |
| Backend language | TypeScript on Node, through the generated `createBackend`. The Dart package is a client SDK; there is no Dart or Rust-hosted backend. | Runs the same Rust engine inside the Node addon. |
| Prerequisite expressions | `@requires(Name(field: self))` is the only supported form: every argument is `self`, the value of the annotated field. The runner that satisfies prerequisites is client code. | Never sees prerequisites; they gate when the client sends a mutation, not what the backend receives. |

These are accepted limits of the current runtime, not planned features. See [deployment](deployment.md) for the process and network boundaries.

## Handlers

```ts
// handlers.ts
import type { Prisma } from '@prisma/client';
import { MutationRejected, type Handlers } from './generated/backend.ts';

export const handlers: Handlers<Prisma.TransactionClient> = {
  async edit({ input, tx, userId, notify }) {
    // In your application, check userId's write permission here.
    const { identity, patch } = input.entry;
    if (patch.text === 'reject') throw new MutationRejected('entry.denied');
    await tx.entry.update({
      where: identity,
      data: {
        ...patch,
        ...(typeof patch.text === 'string' ? { text: patch.text.trim() } : {}),
      },
    });
    notify({ channel: 'book:demo', records: [input.entry] });
  },
};
```

This reproduces the demo's normalization and rejection behavior. It is not an application permission policy; the demo trusts its development user.

`HandlerCall<Tx, Input>` contains:

| Field | Meaning |
| --- | --- |
| `input` | Generated mutation input, such as `EditInput`; update slots have `identity` and `patch` |
| `tx` | Your database transaction object |
| `userId` | Authenticated caller; use it for business authorization |
| `notify` | Synchronous function for declaring changed records and channels |

A handler returns `Promise<void | { channel: string }>`. Returning void selects the only notified channel as its receipt checkpoint. If several channels were notified, return one of those channels explicitly. No notification causes `handler.no_channel`; several channels without a selection cause `handler.ambiguous_checkpoint`. These are programming errors that abort the batch.

One mutation can have several slots and perform several business writes. Ahead runs it in a savepoint inside the batch transaction. The schema describes the local operation and typed input; it does not require the backend to replay the same database operations. The backend can normalize values or use different tables.

The latest `Edit` version uses `handlers.edit`. Additional retained versions use names such as `editV1`. Keep the handlers required by the generated interface while clients can still send those versions. A known but unsupported version fails before any handler executes.

## Loaders

```ts
// loaders.ts
import type { Prisma } from '@prisma/client';
import type { Loaders } from './generated/backend.ts';

export const loaders: Loaders<Prisma.TransactionClient> = {
  async entry({ ids, tx, userId, channel }) {
    // Replace this demo policy with your application's authorization rules.
    if (userId !== 'demo-user' || channel !== 'book:demo') {
      return ids.map(() => null);
    }
    return Promise.all(ids.map(identity => tx.entry.findUnique({ where: identity })));
  },
};
```

`LoaderCall<Tx, Identity>` contains:

| Field | Meaning |
| --- | --- |
| `ids` | Read-only list of typed record identities |
| `tx` | Your transaction, shared with sync persistence for this request |
| `userId` | Caller whose visibility must be checked |
| `channel` | Channel whose synchronization requested these records |

A loader returns `Promise<readonly (Record | null)[]>`. Return exactly one item per identity, in the same order. Do not filter out missing rows or return a differently ordered database result directly.

What each item may be:

| Item | Meaning | Result |
| --- | --- | --- |
| A row object | The record's current state for this user on this channel | Delivered with the stamp stored by the record's publication |
| `null` | The record does not exist, or this user must not see it on this channel | Delivered as a deletion. A newer stamp clears the authoritative row even if another channel still claims it; pending local operations are replayed on that state. |
| `undefined`, a missing entry, a non-array result | A defect | The pull fails with `500 server` and `onError`; the client's cursor does not move |

A row object must match the generated model type exactly. Include every non-identity field: a nullable field that is absent reads as `null`, but an absent non-nullable field is a defect. The identity fields may be present. Any other property, such as an extra database column or a relation object, is a defect. Map your rows to the model type rather than returning a wider database row.

Loaders run during synchronization, not when the app calls local `get`, `query` or `watch`. A malformed result or thrown error fails the request; the backend does not silently skip the failed loader result and advance its cursor.

## Notifications

`notify({ channel, records })` declares changed identities. It does not broadcast the supplied object's field values. A later loader call determines the current content visible to the subscriber.

```ts
import { Entry } from './generated/backend.ts';

notify({ channel: 'book:demo', records: [Entry({ id: 'entry-1' })] });
```

| Interface | Shape |
| --- | --- |
| `RecordRef` | `{ model: string, identity: object }` |
| `NotifyArgs` | `{ channel: string, records: readonly (RecordRef | object)[] }` |
| Generated model reference function | `Entry(identity: EntryIdentity): RecordRef` |
| Handler `notify` | `(args: NotifyArgs) => void` |

The channel must be nonblank. Decoded handler slots such as `input.entry` carry record-reference metadata and can be passed directly. Spreading or cloning a slot can lose this metadata; use the generated model reference function when constructing a reference yourself. The low-level `RECORD` symbol marks these decoded references; applications normally do not need to manipulate it.

Notify every channel that distributes the changed record, including when its loader should now return null. Ahead does not infer changes from arbitrary writes to your database. Multiple calls are allowed, but a successful handler must notify at least one channel.

Each notification allocates a per-record **stamp** and a channel **cursor**. Stamps prevent older content from another channel from overwriting newer content. When channels return different views of one identity, notification order therefore matters. Channels are not isolated copies of the same record. See [concepts](../concepts.md).

## Authentication

`Authenticate` receives Node's `IncomingMessage` and returns a user ID string, null, undefined, or a promise of those values. Null/undefined or a blank user ID rejects authentication. The SDK calls it for HTTP requests and WebSocket connections. Verify your application's session/token here; enforce read permissions in loaders and write permissions in handlers.

`devAuth(): Authenticate` treats `Authorization: Bearer <userId>` as the identity without verification. It is provided for local development, not production authentication. See [authentication and account changes](../frontend/sync.md#authentication-and-account-changes).

## Errors

| Interface | Use |
| --- | --- |
| `new MutationRejected(code)` | Reject a business operation with a stable machine-readable code |
| `translateRejection(error)` | Return a stable rejection code for a known application error; return null/undefined for other errors |
| `onError(error)` | Log server failures that are returned to the client as a generic server error |
| `EngineError` | A failure from the native engine: `code` (stable), `message` (readable, may change), `details` (fields the code promises) |

Codes must match `^[a-z][a-z0-9]*(?:[._-][a-z0-9]+)*$`, such as `entry.denied`. An invalid code is itself an error. A recognized business rejection rolls back that mutation's business writes and notifications and is included in the receipt. The client rolls back its optimistic change and retains a rejection entry. A network error is not a business rejection and must not cause a duplicate business action.

Unexpected exceptions abort the batch transaction. Do not translate every exception into a rejection: a database outage or programming error should remain a retryable request failure. `onError` receives failures including authentication exceptions, persistence faults, checkpoint errors and live-drain failures.

Protocol refusals are answered with a status and a JSON body chosen by the engine error's `code`. Rewording a message never changes a status.

| Code | HTTP status | Meaning |
| --- | --- | --- |
| `request.invalid` | 400 | Malformed body, or a pull cursor ahead of the channel head |
| `client.owner_mismatch` | 403 | The client identity belongs to another user |
| `gap`, `overlap` | 409 | The batch sequence is not the next one and not a retry of the last |
| `mutation_version_unsupported` | 409 | A mutation version this backend does not serve; the body adds `ordinal`, `name` and `version` |
| anything else | 500 `{ code: "server" }` | A server-side failure; the `EngineError` or thrown error goes to `onError` |

## Listener

`await backend.listen({ port, host? })` binds a Node HTTP and WebSocket server. Host defaults to `127.0.0.1`; port zero selects an available port. The result is `{ url, close(): Promise<void> }`.

| Route | Purpose |
| --- | --- |
| `POST /sync/mutations` | Receive mutation batches |
| `POST /sync/pull` | Materialize changed records through loaders for catch-up and gap recovery |
| `/sync/live` (WebSocket) | Subscribe to channels and stream ongoing record changes |

The listener has no TLS, CORS or proxy-header handling and binds to loopback by default; run it behind a reverse proxy as described in [Deploy the backend](deployment.md).

Generated clients use all three routes automatically from one `server` configuration. The WebSocket subscription acknowledgement confirms that channel listeners are installed before HTTP catch-up starts, so changes during catch-up can be queued and reconciled. Listener errors reject. `await server.close()` releases the listener and its live connections; your application must separately close its database pool. The supported listener owns its server; mounting into an application-owned HTTP server is not currently exposed.

## Background writes

Writes outside handlers also need notification. `await backend.notify(tx, args)` stores a one-shot notification inside your existing transaction. This alone does not signal a later commit to live sessions.

For commit-aware wakeups, use a bound session:

```ts
const afterCommit = await database.transaction(async tx => {
  const session = backend.bindTransaction(tx);
  try {
    await tx.entry.update({ where: { id: 'entry-1' }, data: { text: 'From a job' } });
    await session.notify({
      channel: 'book:demo', records: [Entry({ id: 'entry-1' })],
    });
    await session.assertCommittable();
    return session.afterCommit();
  } finally {
    session.close();
  }
});
afterCommit();
```

Here `database` is the same `Database<Tx>` adapter passed to `createBackend`, and `Entry` is the generated reference function. The transaction runner must resolve only after committing.

| Bound-session method | Contract |
| --- | --- |
| `notify(args)` | Await persistence of notification in the supplied transaction |
| `assertCommittable()` | Await/check pending work; failure must abort the transaction |
| `afterCommit()` | Capture a zero-argument wakeup callback; call it only after the database commits |
| `close()` | Release the bound session, including on rollback |

Unlike handler `notify`, external `notify` is asynchronous and must be awaited. Never invoke the commit callback if the transaction fails. Wakeups are process-local; distributed wake delivery needs additional application infrastructure.

## Extension points

`loaderHooks` maps model names to `{ prepareForViewer(call): Promise<void> }`. The hook runs before that model's loader in the same request context. Its failure fails the load. Use it only if viewer-specific preparation is needed; a loader already receives user and channel context.

`native?: Native` injects the native bridge when packaging it elsewhere. It implements `validateConfig`, `processPush`, `processPull`, `publish`, `negotiateLive` and `pullLive` with the string/JSON callback contracts in the [SDK source](https://github.com/zanminwang/ahead/blob/main/packages/server/index.mts). The default binding comes from this repository's Node addon. This is a packaging seam; the generated handlers and loaders remain the application contract.

Backend methods marked `@internal` (`push`, `pull`, `negotiateLive`, `pullLive`, `onCommitted`, `notifyCommitted`, `closeLive`) are used by the listener and tests. They are not the supported application-facing HTTP integration surface.
