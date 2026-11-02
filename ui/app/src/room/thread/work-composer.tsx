// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering."
// while busy, and stands for an error by its code: a lost stream (with Try again when its code is retryable), then a
// refused command, then "Answering.", then an answer Claude stopped giving (thread-break.ts). The stopped question comes
// back into the field when the field is empty. Each conversation keeps its own words (the drafts, by its key); on a new
// conversation the status line offers the plan to ask with, until its first question fixes it, and otherwise names the
// plan the conversation uses. `/` opens the menu of the agent's commands, as the thread's header lists them.
import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import {
  Composer,
  type ComposerHandle,
  type PlanChoice,
  type SlashCommand,
} from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { EntryId, ErrorCode } from "../../transport/protocol.ts";
import type { Conversations } from "../conversations.ts";
import { OtherWait } from "../queue.tsx";
import type { BreakLine } from "../thread-break.ts";
import { useAgentPlan } from "./agent-plan.ts";
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
  // the field holds the words, not React: each change goes straight to the conversation's draft
  const keep = (text: string) => {
    const isEmpty = text.trim() === "";
    if (emptyNow.current !== isEmpty) {
      emptyNow.current = isEmpty;
      setEmpty(isEmpty);
    }
    if (draftKey === null) return;
    if (text === "") drafts.delete(draftKey);
    else drafts.set(draftKey, text);
  };
  // Enter comes here busy or empty too, and sends nothing then: it never breaks the line
  const ask = (asked: string) => {
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
  // U7's Queue: the field's words, sent as Ask sends them
  const queueNow = () => ask(field.current?.text() ?? "");
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
      : busy
        ? { state: answering, testId: "working" }
        : stopped);
  return (
    <Composer
      ref={field}
      took={null}
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
          <OtherWait
            transport={others.transport}
            conversations={others.conversations}
            conversationKey={draftKey}
            busy={busy}
            empty={empty}
            onQueue={queueNow}
            onRefused={others.onRefused}
          />
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
