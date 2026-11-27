// The handover's divider, just before the lawyer's question it went with (handoff.ts finds it): a quiet centred line
// between hairlines, "Context handoff · Claude Opus 5.5 → ChatGPT 6.2 Sol" naming only what is known (UC-1130),
// "Context handoff pending" until it settles (UC-1131), and when it failed, red with the failed mark: "Context not
// restored" (UC-1400), with Try again where sending the question again carries the handover (handoff.ts,
// canTryAgain). Under it, behind "What was passed on" (UC-1401), what the new agent was given (UC-1402..1404).
import { useId, useRef, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconFailed, IconTryAgain } from "@clauseo/components/icons/icons.tsx";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useViewStore } from "../../store/hooks.ts";
import type { HandoffEntry, MessageId, StreamId } from "../../transport/protocol.ts";
import { endText, useHandoff } from "./handoff.ts";
import { questionText } from "./live.ts";
import { useThreadActions } from "./session.ts";

const LINE = "h-px flex-1 bg-divide";

/** What was passed on: the earlier answers, the parts given, and those left out for lack of room (when any). */
function PassedOn({ entry, id }: { entry: HandoffEntry; id: string }) {
  const w = useThreadWords();
  const n = (count: number) => ({ n: String(count) });
  return (
    <p
      id={id}
      data-testid="handoff-passed-on"
      className="mt-(--s1) text-center text-small text-quiet"
    >
      <Say>{w.th("Earlier answers: {n}", n(entry.answers_covered))}</Say>
      {" · "}
      <Say>{w.th("Parts of the conversation passed on: {n}", n(entry.items_included))}</Say>
      {entry.items_left_out > 0 && (
        <>
          {" · "}
          <Say>{w.th("Left out for lack of room: {n}", n(entry.items_left_out))}</Say>
        </>
      )}
    </p>
  );
}

/** The divider for one handoff. `onTryAgain`: present only where Try again redoes it. */
export function HandoffDivider({
  entry,
  onTryAgain,
  going = false,
}: {
  entry: HandoffEntry;
  onTryAgain?: (() => void) | undefined;
  going?: boolean;
}) {
  const w = useThreadWords();
  const [open, setOpen] = useState(false);
  const detail = useId();
  const failed = entry.status === "failed";
  const label =
    entry.status === "pending"
      ? w.th("Context handoff pending")
      : failed
        ? w.th("Context not restored")
        : w.th("Context handoff");
  return (
    <div data-testid="handoff-divider" data-state={entry.status} className="my-(--s4)">
      <div
        className={`flex items-center gap-(--s3) text-small ${failed ? "text-red-text" : "text-quiet"}`}
      >
        <span aria-hidden className={LINE} />
        <span className="flex max-w-[80%] items-center gap-(--s2) text-center">
          {failed && <IconFailed size={14} />}
          <span data-testid="handoff-line">
            <Say>{label}</Say>
            {" · "}
            <span lang="en">{endText(entry.from)}</span>
            {" → "}
            <span lang="en">{endText(entry.to)}</span>
          </span>
        </span>
        <span aria-hidden className={LINE} />
      </div>
      <div className="flex items-center justify-center gap-(--s2)">
        <Button
          kind="quiet"
          size="small"
          data-testid="handoff-more"
          aria-expanded={open}
          aria-controls={detail}
          onClick={() => setOpen((o) => !o)}
        >
          <Say>{w.th("What was passed on")}</Say>
        </Button>
        {onTryAgain !== undefined && (
          <Button
            kind="quiet"
            size="small"
            data-testid="handoff-try-again"
            aria-disabled={going || undefined}
            onClick={onTryAgain}
          >
            <IconTryAgain size={16} />
            <span className="deva-nudge">{mt(w.lang, "Try again")}</span>
          </Button>
        )}
      </div>
      {open && <PassedOn entry={entry} id={detail} />}
    </div>
  );
}

/** The handoff that goes with the live question `message`, if any; Try again sends the question again. */
export function LiveHandoff({ stream, message }: { stream: StreamId; message: MessageId }) {
  const found = useHandoff(stream, message);
  const store = useViewStore();
  const { resend } = useThreadActions();
  const [going, setGoing] = useState(false);
  // two presses in one moment send one question
  const sending = useRef(false);
  if (found === null) return null;
  const tryAgain = () => {
    if (resend === undefined || sending.current) return;
    const view = store.view(stream);
    const id = `user_message:${message}` as const;
    const asked = view?.entries.get(id)?.user_message;
    if (asked === undefined) return;
    const text = questionText(id, asked, view?.texts);
    if (text.trim() === "") return;
    sending.current = true;
    setGoing(true);
    void resend(text).finally(() => {
      sending.current = false;
      setGoing(false);
    });
  };
  return (
    <HandoffDivider
      entry={found.entry}
      onTryAgain={found.again && resend !== undefined ? tryAgain : undefined}
      going={going}
    />
  );
}
