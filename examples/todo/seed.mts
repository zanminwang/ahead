import type { Prisma } from "./prisma/client/index.js";
import type { createBackend } from "./generated/node/backend.ts";

type Tx = Prisma.TransactionClient;
type Backend = ReturnType<typeof createBackend<Tx>>;

export const CHANNEL = "todo:demo";

export const SEED_USERS = [
  { id: "alice", name: "Alice" },
  { id: "bob", name: "Bob" },
] as const;

export const SEED_TODOS = [
  { id: "seed-1", title: "Buy milk", done: false, createdById: "alice" },
  { id: "seed-2", title: "Book a table", done: false, createdById: "bob" },
  { id: "seed-3", title: "Pick up keys", done: false, createdById: "alice" },
] as const;

/**
 * Creates the demo users and tasks when they are missing and publishes them on
 * `todo:demo` in one backend transaction, so connected phones are woken once
 * it commits. Existing rows are left untouched, so an ordinary restart never
 * resets edits made through the app.
 */
export async function seed(backend: Backend): Promise<void> {
  await backend.transaction(async ({ tx, changes, publish }) => {
    for (const user of SEED_USERS) {
      await tx.user.upsert({
        where: { id: user.id },
        create: user,
        update: {},
      });
      changes.add({ model: "User", identity: { id: user.id } });
    }
    for (const todo of SEED_TODOS) {
      await tx.todo.upsert({
        where: { id: todo.id },
        create: todo,
        update: {},
      });
      changes.add({ model: "Todo", identity: { id: todo.id } });
    }
    publish({ channel: CHANNEL });
  });
}
