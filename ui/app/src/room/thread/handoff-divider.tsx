// The handover's divider, just before the lawyer's question it went with (handoff.ts finds it): a quiet centred line
// between hairlines, "Context handoff · Claude Opus 5.5 → ChatGPT 6.2 Sol" naming only what is known (UC-1130). It
// stays pending through the new agent's whole first answer (delivered only once that answer ends without an error),
// so pending reads the same quiet line with a small "Passing on" mark (UC-1406), never an alarm. When it failed: red
// with the failed mark, "Context not restored" (UC-1400), and where the failed turn's own Try again carries the
// handover (handoff.ts, canTryAgain), "Try again sends your question with it." (UC-1405); no button of its own.
// Behind "What was passed on" (UC-1401), what the new agent was given (UC-1402..1404). The model switch's divider
// (ModelSwitchDivider) shares its line.
import { useId, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconFailed } from "@clauseo/components/icons/icons.tsx";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HandoffEntry, MessageId, StreamId, TurnId } from "../../transport/protocol.ts";
import { endText, useHandoff } from "./handoff.ts";
import { useModelSwitch } from "./model-switch.ts";

const LINE = "h-px flex-1 bg-divide";

/** The model switch: the earlier and this answer's model and effort, only what changed (model-switch.ts), data only. */
export function ModelSwitchDivider({ from, to }: { from: string; to: string }) {
  return (
    <div
      data-testid="model-switch-divider"
      className="my-(--s4) flex items-center gap-(--s3) text-small text-quiet"
    >
      <span aria-hidden className={LINE} />
      <span lang="en" className="max-w-[80%] text-center">
        {from} → {to}
      </span>
      <span aria-hidden className={LINE} />
    </div>
  );
}

/** What was passed on: the earlier answers, the parts given whole, and those left out or only in part for lack of
 * room (when any; the host counts shortened results there, items_left_out, and sends no count of its own for them). */
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
      <Say>{w.th("Passed on in full: {n}", n(entry.items_included))}</Say>
      {entry.items_left_out > 0 && (
        <>
          {" · "}
          <Say>{w.th("Left out or shortened for lack of room: {n}", n(entry.items_left_out))}</Say>
        </>
      )}
    </p>
  );
}

/** The divider for one handoff. `again`: the failed turn's Try again carries the handover, so the line says so. */
export function HandoffDivider({ entry, again = false }: { entry: HandoffEntry; again?: boolean }) {
  const w = useThreadWords();
  const [open, setOpen] = useState(false);
  const detail = useId();
  const failed = entry.status === "failed";
  const label = failed ? w.th("Context not restored") : w.th("Context handoff");
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
          {entry.status === "pending" && (
            <span data-testid="handoff-passing" className="flex items-center gap-(--s1)">
              <span
                aria-hidden
                className="size-1.5 rounded-full bg-quiet motion-safe:animate-pulse"
              />
              <Say>{w.th("Passing on")}</Say>
            </span>
          )}
        </span>
        <span aria-hidden className={LINE} />
      </div>
      <div className="flex flex-wrap items-center justify-center gap-x-(--s2)">
        {failed && again && (
          <span data-testid="handoff-again" className="text-small text-red-text">
            <Say>{w.th("Try again sends your question with it.")}</Say>
          </span>
        )}
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
      </div>
      {open && <PassedOn entry={entry} id={detail} />}
    </div>
  );
}

/** The handoff that goes with the live question `message`, if any. */
export function LiveHandoff({ stream, message }: { stream: StreamId; message: MessageId }) {
  const found = useHandoff(stream, message);
  if (found === null) return null;
  return <HandoffDivider entry={found.entry} again={found.again} />;
}

/** The model switch above the live question of `turn`, if its answer ran on another model or effort. */
export function LiveModelSwitch({ stream, turn }: { stream: StreamId; turn: TurnId }) {
  const change = useModelSwitch(stream, turn);
  return change === undefined ? null : <ModelSwitchDivider from={change.from} to={change.to} />;
}
