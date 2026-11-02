// The room's composer as the thread drives it: Ask, or Stop while a turn runs; Enter sends (the composer's own, through
// composer-keys.ts's sendsOnEnter: Shift+Enter breaks the line, and an input method's composing Enter is its own); the
// field keeps its words until the host accepts the question, and after a refusal. The working line says "Answering." while busy, and
// stands for an error by its code: a lost stream (with Try again when its code is retryable), then a refused command,
// then "Answering.", then an answer Claude stopped giving (thread-break.ts). The stopped question comes back into the
// field when the field is empty.
import { useEffect, useRef } from "react";
import { Composer, type ComposerHandle } from "@clauseo/components/clauseo/composer.tsx";
import type { WorkingState } from "@clauseo/components/clauseo/working-line.tsx";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { frame } from "@clauseo/components/lib/words-frame.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { EntryId, MatterId } from "../../transport/protocol.ts";
import type { BreakLine } from "../thread-break.ts";
import type { Drafts } from "./session.ts";

export function WorkComposer({
  drafts,
  matter,
  disabled,
  busy,
  refused,
  failed,
  stopped = null,
  refill = null,
  onAsk,
  onStop,
}: {
  /** The room's drafts, by matter: the field starts with this matter's and keeps it there as it changes. */
  drafts: Drafts;
  matter: MatterId | null;
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
  // the field holds the words, not React: each change goes straight to the matter's draft
  const keep = (text: string) => {
    if (matter === null) return;
    if (text === "") drafts.delete(matter);
    else drafts.set(matter, text);
  };
  // Enter comes here busy or empty too, and sends nothing then: it never breaks the line
  const ask = (asked: string) => {
    if (disabled || busy || asked.trim() === "") return;
    void onAsk(asked).then((ok) => {
      if (!ok) return;
      // words typed while it went keep their place; the emptied field's change drops the draft, and Cmd/Ctrl+Z
      // brings the question back
      if (field.current?.text() === asked) field.current.clear();
      // and an accepted question is no draft: the drafts outlive this composer, so this holds when the lawyer has
      // gone to another matter while the question went
      if (matter !== null && drafts.get(matter) === asked) drafts.delete(matter);
    });
  };
  // the stopped question, once per break, and only into an empty field: words typed since keep their place
  const refilled = useRef<EntryId | null>(null);
  const refillKey = refill?.key ?? null;
  const refillText = refill?.text ?? "";
  useEffect(() => {
    if (refillKey === null || refilled.current === refillKey) return;
    refilled.current = refillKey;
    if (field.current?.text() === "") field.current.setText(refillText);
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
      // keyed by the matter, so another matter's composer starts from that matter's draft
      key={matter ?? "none"}
      ref={field}
      took={null}
      plan=""
      hint={null}
      working={line?.state ?? null}
      opening
      busy={busy}
      onStop={onStop}
      initialText={matter === null ? "" : (drafts.get(matter) ?? "")}
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
