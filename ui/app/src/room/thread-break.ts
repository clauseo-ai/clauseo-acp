// When something breaks in the open thread: an answer Claude stopped giving (limits/stopped), and a stream that failed.
// Each becomes one state of the working line over the composer, chosen by its code and never by HostError.message.
//
// A break is the thread's last turn ending failed, with no question asked since: the step's `failure` says which. The
// line stands only for a break this window saw arrive, so a reopened thread shows its step row's words alone. Until
// the host can restart an agent (H10, a NextHost part), the line offers no Restart: the stopped question goes back
// into an empty field instead, and Ask relaunches the agent through send_prompt.
import { useCallback, useState, useSyncExternalStore } from "react";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useStreamPart } from "../store/hooks.ts";
import type { ViewStore } from "../store/store.ts";
import type { StreamView } from "../store/view.ts";
import { toHostError } from "../transport/host-transport.ts";
import type { EntryId, HostError, StepEntry, StreamId, ThreadId } from "../transport/protocol.ts";
// a type alone: erased, so the first paint has no static dependency on the lines, which come with the thread's chunk
import type * as Lines from "./break-lines.ts";
import type { RestartHost } from "./host-next.ts";
import { latestQuestion } from "./thread/ids.ts";
import { questionText } from "./thread/live.ts";

/** The codes Restart can mend, once the host has it: a restart can't mend a protocol error or an unsupported version. */
export const RESTARTS = new Set<string>([
  "agent.exited",
  "agent.launch_failed",
  "thread.interrupted",
]);

/** Whether a step's stop takes the mark where it stopped (stopped-mark.tsx) rather than its code's sentence. */
export const takesMark = (step: StepEntry): boolean =>
  step.stop === "failed" && step.failure === TIMED;

/** The one code whose stop has a true clock time: the step's `ts` is when the host saw the agent exit. An interrupted
 * turn's step is written by start-up repair, so its time is the next start, not the stop. */
export const TIMED = "agent.exited";

/** The thread's last turn, ended failed with nothing asked since: its step, its code, and the question it answered. */
export type Break = {
  readonly id: EntryId;
  readonly step: StepEntry;
  /** The step's failure, or "unknown" when it gave none. */
  readonly code: string;
  /** The stopped question's words, for the field; "" when the view lacks it. */
  readonly question: string;
  /** When this window first read it, for its time's day ("Monday 4:31 pm"): no clock runs to move it. */
  readonly seen: number;
};

const ENDS = ["step:", "limit_stop:", "user_message:"];
/** The last id that ends or starts a turn, and the latest question, per id list: the view replaces its id list on every
 * add or remove and never changes one in place, so a flush that only appends text reads this without a scan. */
const lastEnds = new WeakMap<
  readonly EntryId[],
  { end: EntryId | null; question: EntryId | null }
>();

function endsOf(ids: readonly EntryId[]) {
  let known = lastEnds.get(ids);
  if (known === undefined) {
    let end: EntryId | null = null;
    for (let i = ids.length - 1; i >= 0 && end === null; i -= 1)
      if (ENDS.some((p) => ids[i]!.startsWith(p))) end = ids[i]!;
    known = { end, question: latestQuestion(ids) };
    lastEnds.set(ids, known);
  }
  return known;
}

/** One Break per step object (an upsert makes a new one), so the pick keeps its identity while nothing changes. */
const breaks = new WeakMap<StepEntry, Break>();

/** The open thread's break: undefined before its view, null when its last turn didn't fail. Stable while unchanged. */
export function pickBreak(view: StreamView | undefined): Break | null | undefined {
  if (view === undefined) return undefined;
  const { end, question } = endsOf(view.ids);
  if (end === null || !end.startsWith("step:")) return null;
  const step = view.entries.get(end)?.step;
  if (step === undefined || step.stop !== "failed") return null;
  let found = breaks.get(step);
  if (found === undefined) {
    const asked = question === null ? undefined : view.entries.get(question)?.user_message;
    found = {
      id: end,
      step,
      code: step.failure ?? "unknown",
      question:
        question === null || asked === undefined ? "" : questionText(question, asked, view.texts),
      seen: Date.now(),
    };
    breaks.set(step, found);
  }
  return found;
}

/** The open thread's break, and whether it arrived while this window watched (a reopened thread's is not). */
export function useThreadBreak(
  store: ViewStore,
  stream: StreamId | null,
): { brk: Break | null; live: boolean } {
  const brk = useStreamPart(store, stream, pickBreak);
  // the break the view held when it first came, per stream: that one was already there. Set while rendering, as
  // React allows for state that follows a prop: React renders again at once, before anything is drawn
  const [opened, setOpened] = useState<{ stream: StreamId; id: EntryId | null } | null>(null);
  if (stream !== null && brk !== undefined && opened?.stream !== stream)
    setOpened({ stream, id: brk?.id ?? null });
  const found = brk ?? null;
  const live = found !== null && opened?.stream === stream && opened.id !== found.id;
  return { brk: found, live };
}

/** A working line state and the test id it stands under. */
export type BreakLine = { state: WorkingState; testId: string };

/** The stopped question for an empty field, a key per break so it comes back once: only before the host restarts
 * (H10), since Restart asks it itself and a question left in the field would be asked twice; only for a code that
 * asking again can mend; and never for a reopened thread's break. */
export function refillOf(
  brk: Break | null,
  live: boolean,
  restart: RestartHost | null,
): { key: EntryId; text: string } | null {
  return brk !== null && live && restart === null && RESTARTS.has(brk.code) && brk.question !== ""
    ? { key: brk.id, text: brk.question }
    : null;
}

/** The break's lines (break-lines.ts), which arrive with the thread's chunk: stopped-mark.tsx, in that chunk, loads
 * them, and they hand themselves over here. A break or a lost stream comes only on an open thread, which opens only
 * once that chunk is in (session.ts), so they are here by then; the first paint carries none of them. */
export type BreakLines = Pick<typeof Lines, "stoppedLine" | "streamFailedLine">;
let provided: BreakLines | null = null;
const heard = new Set<() => void>();

/** break-lines.ts's hand-over, as its module runs. */
export function provideBreakLines(lines: BreakLines): void {
  provided = lines;
  for (const listener of heard) listener();
}

const hear = (listener: () => void) => {
  heard.add(listener);
  return () => void heard.delete(listener);
};
const read = () => provided;

/** The lines, once the thread's chunk is in: null before. */
export const useBreakLines = (): BreakLines | null => useSyncExternalStore(hear, read, read);

/** The open thread's stopped line, with Restart once the host has it: null while a turn runs, for a reopened thread's
 * break, and once Restart is accepted. A refused Restart shows its own code's words. An interrupted turn is written by
 * start-up repair, so it is in the view before the window opens it: with Restart it counts as live, since Restart is
 * its way on. */
export function useStoppedLine({
  lang,
  brk,
  live,
  busy,
  thread,
  restart,
  lines,
  plan,
}: {
  /** The lines' chunk, once loaded. */
  lines: BreakLines | null;
  /** The thread's plan, for the words that name it (agent-plan.ts). */
  plan?: string | null | undefined;
  lang: Lang;
  brk: Break | null;
  live: boolean;
  busy: boolean;
  thread: ThreadId | null;
  restart: RestartHost | null;
}): BreakLine | null {
  // per break: Restart pressed (and not refused), or its refusal
  const [press, setPress] = useState<{ id: EntryId; refused: HostError | null } | null>(null);
  const id = brk?.id ?? null;
  const onRestart = useCallback(() => {
    if (restart === null || thread === null || id === null) return;
    setPress({ id, refused: null });
    restart.restartAgent(thread).catch((e: unknown) => setPress({ id, refused: toHostError(e) }));
  }, [restart, thread, id]);
  const pressed = press !== null && press.id === id ? press : null;
  const shown = live || (restart !== null && brk?.code === "thread.interrupted");
  if (
    lines === null ||
    brk === null ||
    !shown ||
    busy ||
    (pressed !== null && pressed.refused === null)
  )
    return null;
  return lines.stoppedLine(
    lang,
    brk,
    brk.seen,
    restart === null ? null : { onRestart, refused: pressed?.refused ?? null },
    plan,
  );
}
