import * as FileSystem from "expo-file-system/legacy";
import type { Todo } from "../../generated/mobile/client";
import type { LaunchConfig } from "./config";
import { openTodoSession, type OpenedSession } from "./todo";

/**
 * Test-only diagnostics driven by integration/platform/run_todo_ios_smoke.sh through
 * Documents/config.json. They use the same session as the screen and record JSON results;
 * they are not part of the product screen.
 */
const documents = FileSystem.documentDirectory!;

async function writeResult(result: object) {
  const temporary = `${documents}result.tmp`;
  await FileSystem.writeAsStringAsync(temporary, JSON.stringify(result));
  await FileSystem.moveAsync({ from: temporary, to: `${documents}result.json` });
}
function check(condition: unknown, message: string): asserts condition {
  if (!condition) throw Error(message);
}
async function until(predicate: () => Promise<boolean>, message: string) {
  const deadline = Date.now() + 120000;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw Error(`Timed out: ${message}`);
}

export async function runSmoke(config: LaunchConfig, show: (message: string) => void) {
  let opened: OpenedSession | undefined;
  try {
    const { user, phase, expectedClientId } = config;
    show(`${user}: ${phase}`);
    opened = await openTodoSession({
      user,
      url: config.url,
      onConnectionError: (error) => console.log("connection:", String(error)),
    });
    const { session, client } = opened;
    if (expectedClientId)
      check(client.clientId === expectedClientId, "client identity changed across restart");
    let watched: Todo[] = [];
    session.watch(
      (rows) => {
        watched = rows;
      },
      (error) => console.log("watch:", String(error)),
    );
    const todo = (id: string) => client.models.todo.get({ id });
    const pending = async () => (await client.syncState()).pending as number;
    const settled = () => until(async () => (await pending()) === 0, "pending queue drains");
    const seeded = () =>
      until(
        async () =>
          (await client.models.todo.query()).length >= 3 &&
          (await client.models.user.get({ id: user })) !== null,
        "seeded users and tasks",
      );
    if (phase === "online") {
      await seeded();
      if (user === "alice") {
        await session.add("  Alice online task  ");
        const mine = (await client.models.todo.query()).find((row) => row.title === "Alice online task");
        check(mine && mine.done === false && mine.createdById === "alice", "local add visible after commit");
        await until(async () => (await todo(mine.id))?.done === true, "Bob completes Alice's task");
      } else {
        await until(
          async () => (await client.models.todo.query()).some((row) => row.title === "Alice online task"),
          "Alice's task arrives live",
        );
        const theirs = (await client.models.todo.query()).find((row) => row.title === "Alice online task")!;
        await session.setDone(theirs.id, true);
        check((await todo(theirs.id))?.done === true, "local done visible after commit");
      }
      await settled();
    } else if (phase === "offline") {
      check((await client.models.todo.query()).length >= 4, "cached tasks missing offline");
      await session.add("Alice offline task");
      const mine = (await client.models.todo.query()).find((row) => row.title === "Alice offline task")!;
      await session.setDone(mine.id, true);
      await until(
        async () => watched.some((row) => row.id === mine.id && row.done),
        "offline add-then-done visible through the watch",
      );
      check((await pending()) === 2, "offline queue must hold the create and its dependent update");
    } else if (phase === "restart") {
      const mine = (await client.models.todo.query()).find((row) => row.title === "Alice offline task");
      check(mine?.done === true, "offline task lost across process restart");
      check((await pending()) === 2, "queued work lost across process restart");
    } else if (phase === "remote") {
      await session.add("Bob remote task");
      await settled();
    } else if (phase === "settle" || phase === "observe") {
      await until(
        async () => {
          const rows = await client.models.todo.query();
          return (
            rows.some((row) => row.title === "Alice offline task" && row.done) &&
            rows.some((row) => row.title === "Bob remote task") &&
            (await pending()) === 0
          );
        },
        "authoritative convergence",
      );
    } else throw Error(`Unknown phase: ${phase}`);
    const rows = await client.models.todo.query();
    const result = {
      ok: true,
      user,
      phase,
      clientId: client.clientId,
      pending: await pending(),
      rows,
    };
    await writeResult(result);
    show(
      `PASS ${user}: ${phase}\nclient ${client.clientId}\npending ${result.pending}\n${rows
        .map((row) => `${row.done ? "☑" : "☐"} ${row.title}`)
        .join("\n")}`,
    );
    // Keep the connection alive until the runner terminates the process.
  } catch (error) {
    await writeResult({ ok: false, phase: config.phase, error: String(error) });
    show(`FAIL ${String(error)}`);
    await opened?.session.close();
  }
}
