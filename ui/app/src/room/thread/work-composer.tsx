// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering."
// while busy, and stands for an error by its code: a lost stream (with Try again when its code is retryable), then a
// refused command, then "Answering.", then an answer Claude stopped giving (thread-break.ts). The stopped question comes
// back into the field when the field is empty. Each conversation keeps its own words (the drafts, by its key); on a new
// conversation the status line offers the plan to ask with, until its first question fixes it, and otherwise names the
// plan the conversation uses; paused at the plan's limit, it says where the answer stopped (U12). `/` opens the menu
// of the agent's commands, as the thread's header lists them.
import { useCallback, useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import {
  Composer,
  type ComposerHandle,
  type PlanChoice,
  type SlashCommand,
} from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, ErrorCode } from "../../transport/protocol.ts";
import type { Conversations } from "../conversations.ts";
import { OtherWait } from "../queue.tsx";
import { useOtherWait } from "../queue-model.ts";
import type { BreakLine } from "../thread-break.ts";
import { LimitWarning } from "../limits/limit-warning.tsx";
import { useAgentPlan } from "./agent-plan.ts";
import { pausedLine, type Paused } from "./limit-words.ts";
import type { Drafts } from "./session.ts";

export function WorkComposer({
  ref,
  drafts,
  draftKey,
  choice,
  plan = "",
  commands,
  disabled,
  busy,
  refused,
  failed,
  stopped = null,
  refill = null,
  onAsk,
  onStop,
  others = null,
  paused = null,
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
  /** The agent's commands for the `/` menu, from the open thread's header; [] before there are any. */
  commands: readonly SlashCommand[];
  /** No matter is open. */
  disabled: boolean;
  busy: boolean;
  /** The code of a refused command, until the next Ask. */
  refused: string | null;
  /** The lost stream's line, until Try again brings it back. */
  failed: BreakLine | null;
  /** The stopped answer's line, while nothing runs. */
  stopped?: BreakLine | null;
  /** The stopped question, once per break, for an empty field. */
  refill?: { key: EntryId; text: string } | null;
  /** True once the question is accepted. */
  onAsk: (text: string) => Promise<boolean>;
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
}) {
  const w = useWords();
  const agentPlan = useAgentPlan();
  const field = useRef<ComposerHandle>(null);
  useImperativeHandle(
    ref,
    () => ({
      focus: () => field.current?.focus(),
      text: () => field.current?.text() ?? "",
      setText: (text: string) => field.current?.setText(text),
      clear: (only?: string) => field.current?.clear(only),
      fill: (text: string) => field.current?.fill(text),
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
  // Enter comes here busy or empty too, and sends nothing then: it never breaks the line; while another conversation
  // holds the matter, it raises the wait instead, whose Queue sends
  const ask = (asked: string) => {
    if (disabled || busy || asked.trim() === "") return;
    if (wait !== null && !wait.sent) {
      setRaised(true);
      return;
    }
    send(asked);
  };
  const send = (asked: string) => {
    if (disabled || busy || asked.trim() === "") return;
    void onAsk(asked).then((ok) => {
      if (!ok) return;
      // words typed while it went keep their place; the emptied field's change drops the draft, and Cmd/Ctrl+Z
      // brings the question back
      // (while an input method composes, the composer holds the clear for the composition's end)
      field.current?.clear(asked);
      // and an accepted question is no draft: the drafts outlive this composer, so this holds when the lawyer has
      // gone to another conversation while the question went
      if (draftKey !== null && drafts.get(draftKey) === asked) drafts.delete(draftKey);
    });
  };
  // U7's Queue: the field's words, sent as Ask sends them (send_prompt with no `delivery`, the wire's queue)
  const queueNow = () => send(field.current?.text() ?? "");
  // the stopped question, once per break, and only into an empty field: words typed since keep their place, and fill
  // never moves the lawyer's focus or breaks a composition
  const refilled = useRef<EntryId | null>(null);
  const refillKey = refill?.key ?? null;
  const refillText = refill?.text ?? "";
  useEffect(() => {
    if (refillKey === null || refilled.current === refillKey) return;
    refilled.current = refillKey;
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
  const answering: WorkingState = { kind: "wait", words: frame(w.lang, "Answering."), pill: null };
  const line: BreakLine | null =
    failed ??
    (refused !== null
      ? {
          state: {
            kind: "wait",
            words: errorWords(w.lang, refused, agentPlan),
            pill: null,
            code: refused,
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
  return (
    <Composer
      ref={field}
      // the plan's warning under the field (U12), quiet while paused at the limit, when the status line says it:
      // "Stopped at 4:52 pm, at the plan's limit", or the toolbar's at-limit words without the stop's time
      notice={paused === null ? <LimitWarning /> : null}
      took={paused === null ? null : pausedLine(w.lang, paused)}
      plan={plan}
      {...(choice === undefined ? {} : { choice })}
      hint={null}
      commands={commands}
      working={line?.state ?? null}
      opening
      busy={busy}
      onStop={onStop}
      initialText={draftKey === null ? "" : (drafts.get(draftKey) ?? "")}
      onInput={keep}
      onAsk={ask}
      queue={
        others === null ? null : (
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
                onFocusField={() => field.current?.focus()}
                onRefused={others.onRefused}
                stopWanted={isStopWanted}
                onStopWanted={setStopWanted}
              />
            )}
          </>
        )
      }
      disabled={disabled}
      line={false}
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
