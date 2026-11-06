// UD2's notes in the composer's notice line, in the thread's chunk (list.tsx re-exports it): the expired sign-in card
// (the first run's chunk gives it, first-run/hold.ts) when the thread's question is held for it, else the first read's line when the thread is a first read, else the
// note the line had before (U12's warning, U15's out-of-credit). Each reads only its own part of the store.
import { useMemo, useSyncExternalStore, type ReactNode } from "react";
import { NoticeCard } from "@clauseo/components/clauseo/limit-note.tsx";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { AgentId, StreamId, ThreadId, Timestamp, TurnId } from "../../transport/protocol.ts";
import { FirstReadLine } from "../first-read/first-read-line.tsx";
import { readStateOf, type ReadState } from "../first-read/read-state.ts";
import { firstReadMarks, markOfThread, subscribeMarks } from "../first-read/marks.ts";
import { useExpiredCard } from "../first-run/hold.ts";

/** The read's state, one object per change, so the line renders only as it moves. */
function readPick(turns: readonly TurnId[]) {
  let last: ReadState | null = null;
  return (view: StreamView | undefined): ReadState | null => {
    const next = readStateOf(view, turns);
    if (
      next === null ||
      last === null ||
      next.kind !== last.kind ||
      (next.kind === "reading" && last.kind === "reading" && next.title !== last.title)
    )
      last = next;
    return last;
  };
}

const NO_TURNS: readonly TurnId[] = [];

export function AfterSignInNote({
  transport,
  stream,
  thread,
  expired,
  fallback,
}: {
  transport: HostTransport;
  stream: StreamId | null;
  thread: ThreadId | null;
  /** The held question's agent and the failed turn's time, when the card stands. */
  expired: { agent: AgentId; failedAt: Timestamp } | null;
  fallback: ReactNode;
}) {
  // the marks object is the same until one changes; the thread's mark is read from it
  const all = useSyncExternalStore(subscribeMarks, firstReadMarks, firstReadMarks);
  const marked = useMemo(() => markOfThread(thread, all), [all, thread]);
  const turns = marked?.mark.turns ?? NO_TURNS;
  const pick = useReadPick(turns);
  const read = useStreamPart(useViewStore(), marked === null ? null : stream, pick);
  const card = useExpiredCard();
  if (expired !== null && card !== null)
    return card({ transport, agent: expired.agent, failedAt: expired.failedAt, frame: NoticeCard });
  if (marked !== null && read != null && thread !== null)
    return (
      <FirstReadLine
        transport={transport}
        matter={marked.matter}
        thread={thread}
        state={read}
        papers={marked.mark.papers ?? 0}
      />
    );
  return fallback;
}

/** One pick per turn list: a new turn (Restart) makes a new pick. */
const picks = new WeakMap<readonly TurnId[], (view: StreamView | undefined) => ReadState | null>();
function useReadPick(turns: readonly TurnId[]) {
  let pick = picks.get(turns);
  if (pick === undefined) {
    pick = readPick(turns);
    picks.set(turns, pick);
  }
  return pick;
}
