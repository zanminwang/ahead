# Typed API

The typed API has two halves that never meet at runtime but share one compiled schema:

- [Client](client.md) — The generic client runtime for TypeScript and Dart plus the generated model, mutation and transaction classes an application uses.
- [Server](server.md) — The TypeScript backend runtime: `createBackend`, handlers, loaders, publishing and the generated typed signatures.

The compiler emits both halves from the same `.model` files ([Compiler / Generate](../../compiler/generate.md)), so a mutation an application can build on the client is exactly one the server knows how to decode.

## Code map

| Part | Code location |
|---|---|
| Client | [client-js](../../../../../packages/client-js), [dart](../../../../../packages/dart/lib); model-specific classes are compiler output |
| Server | [server/index.mts](../../../../../packages/server/index.mts); typed signatures are compiler output |
