import { useCallback, useEffect, useRef, useState } from "react";
import type { Todo, User } from "../../generated/mobile/client";
import type { LaunchConfig } from "./config";
import { describeRejection, openTodoSession, type OpenedSession } from "./todo";

export type Phase = "loading" | "ready" | "needs-connection";

export interface TodoState {
  phase: Phase;
  user: User | null;
  todos: Todo[];
  error: string | null;
  add(title: string): Promise<void>;
  setDone(id: string, done: boolean): Promise<void>;
}

/** Owns one session for the component lifetime; React state mirrors local watch results only. */
export function useTodos(config: LaunchConfig): TodoState {
  const opened = useRef<OpenedSession | null>(null);
  const recheck = useRef<() => Promise<void>>(async () => {});
  const [phase, setPhase] = useState<Phase>("loading");
  const [user, setUser] = useState<User | null>(null);
  const [todos, setTodos] = useState<Todo[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let disposed = false;
    let unwatch = () => {};
    let loaded = false;
    const connectionFailed = () => {
      // A transport failure never means a local write was lost; it only matters before the first load.
      if (!disposed && !loaded) setPhase("needs-connection");
    };
    void (async () => {
      let session: OpenedSession;
      try {
        session = await openTodoSession({
          user: config.user,
          url: config.url,
          onConnectionError: (failure) => {
            console.log("connection:", String(failure));
            connectionFailed();
            void recheck.current();
          },
        });
      } catch (failure) {
        if (!disposed) setError(failure instanceof Error ? failure.message : "Could not open the local database");
        return;
      }
      if (disposed) {
        await session.session.close();
        return;
      }
      opened.current = session;
      const refreshUser = async () => {
        const row = await session.user();
        if (disposed) return;
        if (row) {
          loaded = true;
          setUser(row);
          setPhase("ready");
        }
      };
      const checkRejections = async () => {
        const rejections = await session.rejections();
        if (disposed || rejections.length === 0) return;
        const latest = rejections[rejections.length - 1]!;
        setError(describeRejection(latest.code));
        console.log("rejected:", JSON.stringify(rejections));
        for (const rejection of rejections) await session.dismiss(rejection.ordinal);
      };
      recheck.current = checkRejections;
      unwatch = session.session.watch(
        (rows) => {
          if (disposed) return;
          setTodos(rows);
          void refreshUser();
          void checkRejections();
        },
        (failure) => console.log("watch:", String(failure)),
      );
      await refreshUser();
    })();
    return () => {
      disposed = true;
      recheck.current = async () => {};
      unwatch();
      const session = opened.current;
      opened.current = null;
      void session?.session.close();
    };
  }, [config.user, config.url]);

  const add = useCallback(async (title: string) => {
    const session = opened.current;
    if (!session) throw Error("The list is still loading");
    await session.session.add(title);
    setError(null);
    await recheck.current();
  }, []);
  const setDone = useCallback(async (id: string, done: boolean) => {
    const session = opened.current;
    if (!session) throw Error("The list is still loading");
    await session.session.setDone(id, done);
    setError(null);
    await recheck.current();
  }, []);

  return { phase, user, todos, error, add, setDone };
}
