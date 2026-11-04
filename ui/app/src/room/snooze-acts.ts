// Snooze and Done's commands (U16b, H-triage), loaded with snooze-late.tsx: one command per thread, and once the host
// has answered Done for every one, the line shown in the row's place; one refused (a time past, or more than 366 days
// ahead) puts back those it took and shows nothing, so the rows stay as they were.
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { CommandEnvelope, ThreadId, Timestamp } from "../transport/protocol.ts";

type Command = CommandEnvelope["command"];

/** What stands in a row's place after a snooze or Done, and the threads its Undo puts back. */
export type Line =
  | { kind: "snoozed"; until: Timestamp; threads: readonly ThreadId[] }
  | { kind: "done"; threads: readonly ThreadId[] };

const warn = (what: string) => (error: unknown) =>
  console.warn(`${what} failed: ${JSON.stringify(toHostError(error).code)}`);

export type Acts = {
  snooze: (key: string, threads: readonly ThreadId[], until: Timestamp) => void;
  done: (key: string, threads: readonly ThreadId[]) => void;
  undo: (line: Line) => void;
  wake: (threads: readonly ThreadId[]) => void;
  notDone: (thread: ThreadId) => void;
};

export function snoozeActs(
  transport: HostTransport,
  show: (key: string, line: Line) => void,
): Acts {
  const send = (command: Command) =>
    transport.dispatch({ command_id: crypto.randomUUID(), command });
  const wake = (thread: ThreadId): Command => ({ snooze_thread: { thread } });
  const undone = (thread: ThreadId): Command => ({ mark_thread_done: { thread, done: false } });
  const each = async (
    key: string,
    threads: readonly ThreadId[],
    make: (thread: ThreadId) => Command,
    back: (thread: ThreadId) => Command,
    line: Line,
  ) => {
    const answers = await Promise.allSettled(threads.map((th) => send(make(th))));
    const refused = answers.find((a) => a.status === "rejected");
    if (refused === undefined) return show(key, line);
    warn(Object.keys(make(threads[0] ?? "")).join())(refused.reason);
    threads.forEach((th, i) => {
      if (answers[i]?.status === "fulfilled") send(back(th)).catch(warn("put back"));
    });
  };
  return {
    snooze: (key: string, threads: readonly ThreadId[], until: Timestamp) =>
      void each(key, threads, (thread) => ({ snooze_thread: { thread, until } }), wake, {
        kind: "snoozed",
        until,
        threads,
      }),
    done: (key: string, threads: readonly ThreadId[]) =>
      void each(key, threads, (thread) => ({ mark_thread_done: { thread, done: true } }), undone, {
        kind: "done",
        threads,
      }),
    undo: (line: Line) => {
      for (const th of line.threads)
        send(line.kind === "done" ? undone(th) : wake(th)).catch(warn("undo"));
    },
    wake: (threads: readonly ThreadId[]) => {
      for (const th of threads) send(wake(th)).catch(warn("snooze_thread"));
    },
    notDone: (thread: ThreadId) => void send(undone(thread)).catch(warn("mark_thread_done")),
  };
}
