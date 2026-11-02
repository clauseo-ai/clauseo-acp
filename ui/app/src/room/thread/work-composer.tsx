// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering." while busy, and
// stands for an error by its code: a lost stream (with Try again when its code is retryable), then a refused command,
// then "Answering.", then an answer Claude stopped giving (thread-break.ts). The stopped question comes back into the
// field when the field is empty. Each conversation keeps its own words (the drafts, by its key); on a new conversation
// the status line offers the plan to ask with, until its first question fixes it.
import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import {
  Composer,
  type ComposerHandle,
  type PlanChoice,
} from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { EntryId } from "../../transport/protocol.ts";
import type { BreakLine } from "../thread-break.ts";
import type { Drafts } from "./session.ts";

export function WorkComposer({
  ref,
  drafts,
  draftKey,
  choice,
  disabled,
  busy,
  refused,
  failed,
  stopped = null,
  refill = null,
  onAsk,
  onStop,
}: {
  /** The composer's handle, for the room to put the focus in the field. */
  ref?: Ref<ComposerHandle>;
  /** The room's drafts, by conversation: the field starts with this one's and keeps it there as it changes. */
  drafts: Drafts;
  /** The open conversation's key in the drafts; null with no matter open. */
  draftKey: string | null;
  /** A new conversation's plan choice, before its first question; absent once fixed, or with one plan. */
  choice?: PlanChoice | undefined;
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
}) {
  const w = useWords();
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
  // the field holds the words, not React: each change goes straight to the conversation's draft
  const keep = (text: string) => {
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
          state: { kind: "wait", words: errorWords(w.lang, refused), pill: null, code: refused },
          testId: "error",
        }
      : busy
        ? { state: answering, testId: "working" }
        : stopped);
  return (
    <Composer
      // keyed by the conversation, so another's composer starts from that one's draft
      key={draftKey ?? "none"}
      ref={field}
      took={null}
      plan=""
      {...(choice === undefined ? {} : { choice })}
      hint={null}
      working={line?.state ?? null}
      opening
      busy={busy}
      onStop={onStop}
      initialText={draftKey === null ? "" : (drafts.get(draftKey) ?? "")}
      onInput={keep}
      onAsk={ask}
      disabled={disabled}
      line={false}
      testIds={{
        field: "composer",
        ask: "send",
        stop: "cancel",
        working: line?.testId ?? "working",
      }}
    />
  );
}
