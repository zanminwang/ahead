# Engine

The server engine is pure protocol logic in Rust: it never opens a connection or a transaction itself, but drives the host through a fixed set of operations.

- [Push](push.md) — Validate and deduplicate mutation batches, invoke handlers and produce receipts.
- [Pull](pull.md) — Find changes by channel cursor and invoke loaders to return records.
- [Notify](notify.md) — Publish changed records to channels at their current stamps and allocate channel cursors.

## How the parts work together

```mermaid
flowchart LR
    B["batch"] --> H["Push<br/>per mutation, in a savepoint:<br/>handle → advanceStamp → load"]
    H -- "publish intents" --> N["Notify<br/>cursor per channel<br/>at the record's stamp"]
    H --> R["receipt<br/>rejections + records @stamp"]
    N --> I[("invalidations")]
    I --> L["Pull<br/>scan after cursor → load"]
    L --> PG["page<br/>changes @stamp"]
```

One stamp per change, allocated in Push; Notify and Pull only carry it.


A handler run by [Push](push.md) writes to the application's database, may report extra changed records (`changes.add`) and may ask for publications (`publish`). When it returns, Push allocates one new version number (the *stamp*) per changed record, reads every changed record back through the application's loaders at the version the client declared, and puts that content, with its stamp, in the receipt. [Notify](notify.md) then carries out the publications: each gives the record a new position (the *cursor*) in one channel at the record's current stamp, stored in the invalidation table; publishing never allocates a stamp. When a client pulls that channel, [Pull](pull.md) scans the invalidation table past the client's cursor, asks the loaders for the current rows and returns them with the records' current stamps. The client completes the mutation from the receipt alone and applies content from every path in stamp order, so a receipt and a page for the same change agree, and the same record can be published to several channels without conflict.

## Code map

| Part | Code location |
|---|---|
| Push | [server/lib.rs](../../../../../crates/server/src/lib.rs) (`process_push`, `decode`); readback in [server/readback.rs](../../../../../crates/server/src/readback.rs) (`read_back`) |
| Pull | [server/lib.rs](../../../../../crates/server/src/lib.rs) (`process_pull`) |
| Notify | publication resolution in [server/readback.rs](../../../../../crates/server/src/readback.rs) (`publish_one`); the external path in [server/lib.rs](../../../../../crates/server/src/lib.rs) (`publish`); `changes`, `publish` and `WakeHub` in [server/index.mts](../../../../../packages/server/index.mts) |
