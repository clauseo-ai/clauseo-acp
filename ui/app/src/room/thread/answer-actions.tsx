// The line under a finished answer (D-026, D-027): Copy, the copy icon (ui/copy-button.tsx), which puts the answer's
// words on the clipboard as written (its Markdown, every part of it between the steps, a blank line apart), and on the thread's latest answer, once it
// was stopped or failed, Try again, which sends the same question as a new turn (its words; a paper it mentioned goes
// as the words that named it). Nothing shows while the turn runs: the line comes with the turn's step. The texts are
// read from the store on the press, so a flush renders nothing here.
import { useRef, useState } from "react";
import { IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { CopyButton } from "@clauseo/components/ui/copy-button.tsx";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { answerText } from "./answer-text.ts";
import { questionText, turnRunning } from "./live.ts";
import { useThreadActions } from "./session.ts";

/** How the turn ended, by its step: whether Try again belongs under it (a Stop, or a failure). */
const retryable = (e: ViewEntry | undefined): boolean =>
  e?.step?.stop === "cancelled" || e?.step?.stop === "failed";

export function AnswerActions({
  ids,
  step,
  question,
  stream,
  paged,
  latest,
}: {
  /** The answer's rows, its texts among them. */
  ids: readonly EntryId[];
  /** The turn's step: the line shows once it is in. */
  step: EntryId;
  /** The question the answer is to, for Try again; none for an exchange without one in the window. */
  question: EntryId | undefined;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  latest: boolean;
}) {
  const w = useWords();
  const store = useViewStore();
  const { resend } = useThreadActions();
  const [going, setGoing] = useState(false);
  // two presses in one moment send one question: the ref holds before React renders the state
  const sending = useRef(false);
  const again = useStreamPart(
    store,
    stream,
    (v) =>
      latest &&
      question !== undefined &&
      !turnRunning(v?.header?.header?.status) &&
      retryable(paged.get(step) ?? v?.entries.get(step)),
  );
  const has = useStreamPart(store, stream, (v) => answerText(ids, v, paged) !== "");
  const retry = () => {
    if (resend === undefined || question === undefined || sending.current) return;
    const view = store.view(stream);
    const asked = paged.get(question)?.user_message ?? view?.entries.get(question)?.user_message;
    if (asked === undefined) return;
    const text = questionText(question, asked, paged.has(question) ? undefined : view?.texts);
    if (text.trim() === "") return;
    sending.current = true;
    setGoing(true);
    void resend(text).finally(() => {
      sending.current = false;
      setGoing(false);
    });
  };
  return (
    <div className="mt-s1 flex flex-wrap items-center gap-x-s2" data-testid="answer-actions">
      {has && (
        <CopyButton
          testId="answer-copy"
          text={() => answerText(ids, store.view(stream), paged)}
          label={th(w.lang, "Copy")}
          done={th(w.lang, "Copied")}
        />
      )}
      {again && resend !== undefined && (
        <Button
          kind="quiet"
          size="small"
          data-testid="answer-try-again"
          aria-disabled={going || undefined}
          onClick={retry}
        >
          <IconTryAgain size={16} />
          <span className="deva-nudge">{mt(w.lang, "Try again")}</span>
        </Button>
      )}
    </div>
  );
}
