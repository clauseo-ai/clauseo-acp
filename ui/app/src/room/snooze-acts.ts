// Snooze and Done's commands (U16b, H-triage), loaded with snooze-late.tsx: one command per thread, and once the host
// has answered Done for every one, the line shown in the row's place; one refused (a time past, or more than 366 days
// ahead) puts back those it took and shows nothing, so the rows stay as they were. Putting a snooze back, by Undo or
// after a refusal, gives each thread the snooze it had before, if that time is still ahead, else none.
import { toHostError, type HostTransport } from "../transport/host-transport.ts";
import type { CommandEnvelope, ThreadId, ThreadSummary, Timestamp } from "../transport/protocol.ts";

type Command = CommandEnvelope["command"];

/** A thread as the acts take it: its id, and the snooze it had before, if any. */
export type Held = Pick<ThreadSummary, "thread" | "snoozed_until">;

/** What stands in a row's place after a snooze or Done, and the threads its Undo puts back. */
export type Line =
  | { kind: "snoozed"; until: Timestamp; threads: readonly Held[] }
  | { kind: "done"; threads: readonly Held[] };

const warn = (what: string) => (error: unknown) =>
  console.warn(`${what} failed: ${JSON.stringify(toHostError(error).code)}`);

export type Acts = {
  snooze: (key: string, threads: readonly Held[], until: Timestamp) => void;
  done: (key: string, threads: readonly Held[]) => void;
  undo: (line: Line) => void;
  wake: (threads: readonly ThreadId[]) => void;
  notDone: (thread: ThreadId) => void;
};

/** A thread's snooze put back: the one it had, while still ahead of `now`; else none. */
export const putBack = ({ thread, snoozed_until: was }: Held, now: Timestamp): Command => ({
  snooze_thread: was !== undefined && was > now ? { thread, until: was } : { thread },
});

export function snoozeActs(
  transport: HostTransport,
  show: (key: string, line: Line) => void,
): Acts {
  const send = (command: Command) =>
    transport.dispatch({ command_id: crypto.randomUUID(), command });
  const back = (th: Held) => putBack(th, Date.now());
  const undone = ({ thread }: Pick<Held, "thread">): Command => ({
    mark_thread_done: { thread, done: false },
  });
  const each = async (
    key: string,
    threads: readonly Held[],
    make: (thread: ThreadId) => Command,
    restore: (th: Held) => Command,
    line: Line,
  ) => {
    const answers = await Promise.allSettled(threads.map((th) => send(make(th.thread))));
    const refused = answers.find((a) => a.status === "rejected");
    if (refused === undefined) return show(key, line);
    warn(Object.keys(make("")).join())(refused.reason);
    threads.forEach((th, i) => {
      if (answers[i]?.status === "fulfilled") send(restore(th)).catch(warn("put back"));
    });
  };
  return {
    snooze: (key, threads, until) =>
      void each(key, threads, (thread) => ({ snooze_thread: { thread, until } }), back, {
        kind: "snoozed",
        until,
        threads,
      }),
    done: (key, threads) =>
      void each(key, threads, (thread) => ({ mark_thread_done: { thread, done: true } }), undone, {
        kind: "done",
        threads,
      }),
    undo: (line) => {
      for (const th of line.threads)
        send(line.kind === "done" ? undone(th) : back(th)).catch(warn("undo"));
    },
    wake: (threads) => {
      for (const thread of threads)
        send({ snooze_thread: { thread } }).catch(warn("snooze_thread"));
    },
    notDone: (thread) => void send(undone({ thread })).catch(warn("mark_thread_done")),
  };
}
