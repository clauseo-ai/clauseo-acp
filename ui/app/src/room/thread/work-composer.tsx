// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering."
// while busy, and stands for an error by its code: a lost stream (with Try again when its code is retryable), then a
// refused command, then "Answering.", then an answer Claude stopped giving (thread-break.ts). The stopped question comes
// back into the field when the field is empty. Each conversation keeps its own words (the drafts, by its key); on a new
// conversation the status line offers the plan to ask with, until its first question fixes it, and otherwise names the
// plan the conversation uses; paused at the plan's limit, it says where the answer stopped (U12). `/` opens the menu
// of the agent's commands, as the thread's header lists them, and `@` the matter's papers (paper-mentions.ts): a send,
// queued or steered too, names each paper its tokens still mention.
// U7: while Clauseo works, Enter queues the field's words (the host queues a send during a turn, H9), and the line above
// the field offers Send now and Queue (composer-steer.tsx); at QUEUE_CAP Enter queues nothing and keeps the words, and a
// send refused as `thread.queue_full` says the cap. The queue's strip sits over the field for every conversation
// with a thread (queue-slot.tsx), under the wait for another conversation (queue.tsx). After the hint's Send now or Queue
// the focus goes back to the field, so the emptied words can be undone. An edit the strip couldn't make because its
// message had already gone comes back into the field (after the words there, a blank line between); when it was the
// last queued, the working line says so until the next send or the drained turn starts (else the strip does).
import {
  use,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
  type Ref,
} from "react";
import {
  Composer,
  type ComposerHandle,
  type Mentions,
  type PlanChoice,
  type SlashCommand,
  type ToolsSegment,
} from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { StoreContext } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, ErrorCode, StreamId, ThreadId } from "../../transport/protocol.ts";
import type { Conversations } from "../conversations.ts";
import { OtherWait } from "../queue.tsx";
import { useOtherWait } from "../queue-model.ts";
import { QueueSlot } from "../queue-slot.tsx";
import type { BreakLine } from "../thread-break.ts";
import { LimitWarning } from "../limits/limit-warning.tsx";
import { OutOfCredit } from "../connectors/out-of-credit.tsx";
import { useAgentPlan } from "./agent-plan.ts";
import { useNow } from "../../store/limits.ts";
import { pausedLine, type Paused } from "./limit-words.ts";
import { pickFull, queueFullLine } from "./composer-queue.ts";
import { ComposerSteer } from "./composer-steer.tsx";
import { asksWords } from "./step-names.ts";
import { useHold } from "../first-run/hold.ts";
import type { AskHow, Drafts } from "./session.ts";
import { ProtectionContext } from "../protection/context.ts";

export function WorkComposer({
  ref,
  drafts,
  draftKey,
  choice,
  plan = "",
  tools = null,
  commands,
  mentions,
  disabled,
  busy,
  doing = null,
  asks = null,
  refused,
  failed,
  stopped = null,
  noteFor,
  gateYields = false,
  refill = null,
  onAsk,
  onStop,
  others = null,
  paused = null,
  transport = null,
  stream = null,
  thread = null,
}: {
  /** The composer's handle, for the room to put the focus in the field. */
  ref?: Ref<ComposerHandle>;
  /** The room's drafts, by conversation: the field starts with this one's and keeps it there as it changes. */
  drafts: Drafts;
  /** The open conversation's key in the drafts; null with no matter open. Fixed for the composer's life: the field
   * reads its draft once, so the host keys this composer by it (room.tsx keys Work so). */
  draftKey: string | null;
  /** A new conversation's plan choice, before its first question; absent once fixed, or with one plan. */
  choice?: PlanChoice | undefined;
  /** The status line's plan words, "Uses your Claude plan"; "" for none. Not drawn while the choice stands. */
  plan?: string;
  /** The tool line after the plan, from the open thread's header's tools (tool-line.tsx); null for none. */
  tools?: ToolsSegment | null;
  /** The agent's commands for the `/` menu, from the open thread's header; [] before there are any. */
  commands: readonly SlashCommand[];
  /** The matter's papers for the `@` menu; none with no matter open, or without H8. */
  mentions?: Mentions | undefined;
  /** No matter is open. */
  disabled: boolean;
  busy: boolean;
  /** What the running step is doing ("Reading Lease.pdf."), said in place of "Answering."; null for none. */
  doing?: string | null;
  /** A permission prompt or a question waiting on the lawyer: said in place of what the step in hand is doing, led by
   * the question mark, the pen stopped; null for none. */
  asks?: "permission" | "question" | null;
  /** The code of a refused command, until the next Ask. */
  refused: string | null;
  /** The lost stream's line, until Try again brings it back. */
  failed: BreakLine | null;
  /** The stopped answer's line, while nothing runs. */
  stopped?: BreakLine | null;
  /** UD2: the notice line's note, given the one it would show otherwise (the expired card, the first read's line). */
  noteFor?: ((fallback: ReactNode) => ReactNode) | undefined;
  /** UD2: the expired card holds the question and offers the sign-in itself: the gate leaves the field's row. */
  gateYields?: boolean;
  /** The stopped question, once per break, for an empty field. */
  refill?: { key: EntryId; text: string } | null;
  /** True once the question is accepted; `how` says whether it goes behind a running turn, or steers it (U7). */
  onAsk: (text: string, how?: AskHow) => Promise<boolean>;
  onStop: () => void;
  /** U7: the matter's conversations, for the wait over the field while another of them holds the matter, whose Queue
   * sends the field's words as Ask does; with a refused Stop the other's code going to `onRefused`. Null: no wait. */
  others?: {
    transport: HostTransport;
    conversations: Conversations;
    onRefused: (code: ErrorCode | null) => void;
  } | null;
  /** U12: the open thread paused at the plan's limit (limit-words.ts's pausedPick), for the status line's words; null
   * while it isn't. Send stays enabled: no limits state holds a question back. */
  paused?: Paused | null;
  /** The host, for the queue's strip; none, no strip. */
  transport?: HostTransport | null;
  /** The open thread's stream and thread, for the queue: null before the conversation has one. */
  stream?: StreamId | null;
  thread?: ThreadId | null;
}) {
  // the view store, read as Enter queues (the cap); none where the composer is mounted alone
  const store = use(StoreContext);
  const w = useWords();
  // the paused line's time against this render's clock (U12)
  const now = useNow();
  const agentPlan = useAgentPlan();
  // UD1: the first run's hold, while the plan isn't signed in: its gate stands in the field's row, and nothing sends
  const hold = useHold(transport);
  // UC: the room's protection line, and the firm's hold on send while the open matter has no protection
  const shield = use(ProtectionContext);
  const gated = hold?.gate !== undefined && !gateYields;
  const field = useRef<ComposerHandle>(null);
  // the words an Ask sent while nothing ran, until the host answers; and the stopped question held for that answer,
  // its turn stopped before the answer cleared the field (the refill below)
  const asking = useRef<string | null>(null);
  const held = useRef<string | null>(null);
  useImperativeHandle(
    ref,
    () => ({
      focus: () => field.current?.focus(),
      text: () => field.current?.text() ?? "",
      setText: (text: string) => field.current?.setText(text),
      clear: (only?: string) => field.current?.clear(only),
      fill: (text: string) => field.current?.fill(text),
      mentioned: () => field.current?.mentioned() ?? [],
    }),
    [],
  );
  // whether the field is empty, for Queue: set only as it fills or empties, so a keystroke renders nothing
  const [empty, setEmpty] = useState(
    () => (draftKey === null ? "" : (drafts.get(draftKey) ?? "")).trim() === "",
  );
  const emptyNow = useRef(empty);
  // U7: another conversation holding the matter, and whether Ask has raised its wait (design state.ts's `waiting`):
  // Ask then sends nothing, Queue sends; emptying the field lowers it
  const wait = useOtherWait(others?.conversations ?? null, draftKey);
  const [raised, setRaised] = useState(false);
  // Queue pressed: Stop the other takes the focus back until the message waits (queue.tsx)
  const stopWanted = useRef(false);
  const isStopWanted = useCallback(() => stopWanted.current, []);
  const setStopWanted = useCallback((wanted: boolean) => {
    stopWanted.current = wanted;
  }, []);
  if (raised && wait === null) setRaised(false);
  // an edit lost as its message went (the strip's onEditLost): the working line says so until the next send, or until
  // the thread moves on (its status, or the agent's latest output: the drained turn has started), when it says
  // "Answering." again. `lost` holds the thread's mark as the edit was lost, null for none; each clear checks it first,
  // so a send never sets the null already there (a second commit in the reply's frame, #253)
  const [lost, setLost] = useState<string | null>(null);
  // the thread's mark, read from the store only while an edit is lost: otherwise no flush renders the composer
  const watched = lost !== null && stream !== null ? stream : null;
  const subscribe = useCallback(
    (on: () => void) => (store === null || watched === null ? noop : store.subscribe(on)),
    [store, watched],
  );
  const readMark = useCallback(
    () => (store === null || watched === null ? null : markOf(store.view(watched))),
    [store, watched],
  );
  const mark = useSyncExternalStore(subscribe, readMark, readMark);
  // moved on: cleared in this render, so the line's change is the flush's own commit, not one after it
  if (lost !== null && mark !== null && mark !== lost) setLost(null);
  // the field holds the words, not React: each change goes straight to the conversation's draft
  const keep = (text: string) => {
    const isEmpty = text.trim() === "";
    if (emptyNow.current !== isEmpty) {
      emptyNow.current = isEmpty;
      setEmpty(isEmpty);
      if (isEmpty) setRaised(false);
    }
    if (draftKey === null) return;
    if (text === "") drafts.delete(draftKey);
    else drafts.set(draftKey, text);
  };
  // Enter comes here empty too, and sends nothing then: it never breaks the line. While Clauseo works it queues the
  // words, but not past the cap, when they stay in the field; while another conversation holds the matter, it raises
  // the wait instead, whose Queue sends
  const ask = (asked: string) => {
    if (disabled || shield?.blocked === true || gated || asked.trim() === "") return;
    if (busy) {
      queue(asked);
      return;
    }
    if (wait !== null && !wait.sent) {
      setRaised(true);
      return;
    }
    send(asked);
  };
  // the field's words behind the running turn: the host queues them (send_prompt with no `delivery`)
  const queue = (asked: string) => {
    if (pickFull(stream === null ? undefined : store?.view(stream))) return;
    send(asked, { behind: true });
  };
  const send = (asked: string, how?: AskHow) => {
    if (disabled || shield?.blocked === true || gated || asked.trim() === "") return;
    // idle, Ask waits for the turn it sent; Queue (another's wait) and the hint's buttons go behind or steer
    if (how === undefined && busy) return;
    if (lost !== null) setLost(null);
    // the papers the field's tokens mention as it goes, read with its words, sent with them however they go
    const files = (field.current?.mentioned() ?? []).map((m) => m.id);
    if (how === undefined) asking.current = asked;
    void onAsk(asked, files.length === 0 ? how : { ...how, files }).then((ok) => {
      if (how === undefined) asking.current = null;
      const back = held.current;
      held.current = null;
      if (!ok) return;
      // pressed on the hint's Send now or Queue, which sit outside the field's row: the focus goes back to the field
      // first, so the clear is one step Cmd/Ctrl+Z takes back, and not lost with the hint as it goes
      if (field.current?.text() === asked && inHint(document.activeElement)) field.current.focus();
      // words typed while it went keep their place; the emptied field's change drops the draft, and Cmd/Ctrl+Z
      // brings the question back
      // (while an input method composes, the composer holds the clear for the composition's end)
      field.current?.clear(asked);
      // and an accepted question is no draft: the drafts outlive this composer, so this holds when the lawyer has
      // gone to another conversation while the question went
      if (draftKey !== null && drafts.get(draftKey) === asked) drafts.delete(draftKey);
      // its turn already stopped (refill below): the question comes back into the field just emptied
      if (back !== null) field.current?.fill(back);
    });
  };
  // U7's Queue: the field's words, sent as Ask sends them (send_prompt with no `delivery`, the wire's queue)
  const queueNow = () => send(field.current?.text() ?? "");
  // the hint's Queue, as Enter; its Send now steers the running answer
  const queueField = () => queue(field.current?.text() ?? "");
  const focusField = useCallback(() => field.current?.focus(), []);
  // the strip's lost edit: into an empty field as it is, else after the words there with a blank line between; never
  // over them
  const editLost = useCallback(
    (text: string, say: boolean) => {
      const f = field.current;
      if (f === null) return;
      const there = f.text();
      if (there === "") f.fill(text);
      else f.setText(`${there}\n\n${text}`);
      // with other messages left, the strip says it had gone
      if (say && lost === null)
        setLost(markOf(stream === null ? undefined : store?.view(stream)) ?? "");
    },
    [lost, store, stream],
  );
  const steerField = () => send(field.current?.text() ?? "", { behind: true, delivery: "steer" });
  // the stopped question, once per break, and only into an empty field: words typed since keep their place, and fill
  // never moves the lawyer's focus or breaks a composition. The host's answer to send_prompt and the turn over the
  // stream come back two ways, in either order: an agent that exits at once can stop the turn while the field still
  // holds the question, before the answer clears it. Then the question waits for that clear and comes back after it
  const refilled = useRef<EntryId | null>(null);
  const refillKey = refill?.key ?? null;
  const refillText = refill?.text ?? "";
  useEffect(() => {
    if (refillKey === null || refilled.current === refillKey) return;
    refilled.current = refillKey;
    if (asking.current === refillText && field.current?.text() === refillText) {
      held.current = refillText;
      return;
    }
    field.current?.fill(refillText);
  }, [refillKey, refillText]);
  const waitTitle = wait?.title ?? mt(w.lang, "New conversation");
  const said =
    wait !== null && raised && !wait.sent && !busy
      ? `${mt(w.lang, "Waiting for '{title}' to finish.", { title: waitTitle })} ${mt(
          w.lang,
          "One conversation changes a matter's papers at a time. Your message is ready to go.",
        )}`
      : "";
  // while the turn runs the pen writes after the words (working-line.tsx); the line is gone, and the pen with it,
  // once the turn ends. A single read stays a wait in its own words: check and series say "Checking" and "judgments",
  // which a plain read isn't
  const answering: WorkingState =
    asks === null
      ? { kind: "wait", words: doing ?? frame(w.lang, "Answering."), pill: null, working: true }
      : { kind: "wait", words: asksWords(w.lang, asks), pill: null, asks };
  const line: BreakLine | null =
    failed ??
    (refused !== null
      ? {
          state: {
            kind: "wait",
            words:
              refused === "thread.queue_full"
                ? queueFullLine(w.lang)
                : errorWords(w.lang, refused, agentPlan),
            pill: null,
            code: refused,
          },
          testId: "error",
        }
      : lost !== null
        ? {
            state: {
              kind: "wait",
              words: mt(w.lang, "That message had already gone."),
              pill: null,
              code: "thread.turn_not_queued",
            },
            testId: "error",
          }
        : wait !== null && (wait.sent || busy)
          ? {
              // sent (or accepted, before the row says starting) and waiting behind the other: the design's wait
              // (room.tsx), never "Answering."
              state: {
                kind: "wait",
                words: mt(w.lang, "Waiting for '{title}' to finish.", {
                  title: waitTitle,
                }),
                pill: null,
              },
              testId: "working",
            }
          : busy
            ? { state: answering, testId: "working" }
            : stopped);
  const credit =
    transport === null ? null : (
      <OutOfCredit transport={transport} conversation={thread ?? draftKey} />
    );
  return (
    <Composer
      ref={field}
      // the plan's warning under the field (U12), quiet while paused at the limit, when the status line says it:
      // "Stopped at 4:52 pm, at the plan's limit", or the toolbar's at-limit words without the stop's time. With no
      // warning, U15's out-of-credit note, hidden by this conversation's thread (or its draft's key before it has one)
      notice={
        shield?.blocked === true
          ? shield.notice
          : (noteFor ?? same)(paused === null ? <LimitWarning fallback={credit} /> : credit)
      }
      took={paused === null ? null : pausedLine(w.lang, paused, now)}
      // under the gate the plan is the gate's to name: the status line says nothing of it
      plan={gated ? "" : plan}
      tools={tools}
      {...(choice === undefined ? {} : { choice })}
      hint={null}
      commands={commands}
      mentions={mentions}
      working={line?.state ?? hold?.working ?? null}
      {...(gated ? { gate: hold?.gate } : {})}
      opening
      busy={busy}
      onStop={onStop}
      initialText={draftKey === null ? "" : (drafts.get(draftKey) ?? "")}
      onInput={keep}
      onAsk={ask}
      acts={
        // while Clauseo works and the field holds words, unless the conversation waits behind another (design
        // room.tsx: `busy && hasText && live.behind === null`)
        busy && !empty && !disabled && wait === null ? (
          <ComposerSteer stream={stream} onSendNow={steerField} onQueue={queueField} />
        ) : null
      }
      queue={
        <>
          {others !== null && (
            <>
              {/* what assistive technology hears of the raised wait: one status region, mounted for the composer's
                life and empty until the wait is raised, so the wait is said as it comes; once sent it is empty
                again, as the composer's own region then says the working line's wait */}
              <p className="sr-only" role="status" data-testid="other-wait-said">
                {said}
              </p>
              {wait !== null && (wait.sent || raised || busy) && (
                <OtherWait
                  transport={others.transport}
                  wait={wait}
                  busy={busy}
                  empty={empty}
                  onQueue={queueNow}
                  onFocusField={focusField}
                  onRefused={others.onRefused}
                  stopWanted={isStopWanted}
                  onStopWanted={setStopWanted}
                />
              )}
            </>
          )}
          {transport !== null && thread !== null && (
            <QueueSlot
              transport={transport}
              stream={stream}
              thread={thread}
              onFocusField={focusField}
              onEditLost={editLost}
            />
          )}
        </>
      }
      disabled={disabled || shield?.blocked === true}
      line={hold?.line ?? (shield === null ? false : shield.line)}
      testIds={{
        field: "composer",
        ask: "send",
        stop: "cancel",
        working: line?.testId ?? "working",
        menu: "composer-menu",
        menuItem: "composer-menu-item",
      }}
    />
  );
}

/** Whether the focus stands on the steer hint's line (its Send now or Queue). */
function inHint(at: Element | null): boolean {
  return at instanceof HTMLElement && at.closest(".steer-hint") !== null;
}

const noop = () => () => {};

/** Where the thread stands, as one string: its status and the agent's latest output. A change means it has moved on
 * from where it stood as an edit was lost. Null with no view. */
function markOf(v: StreamView | undefined): string | null {
  if (v === undefined) return null;
  const status = v.header?.header?.status;
  const said = status === undefined ? "" : typeof status === "string" ? status : "queued";
  return `${said} ${v.latestOutput ?? ""}`;
}

/** The notice line's note as it stands, with no UD2 note over it. */
const same = (note: ReactNode): ReactNode => note;
