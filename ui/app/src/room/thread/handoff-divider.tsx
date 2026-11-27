// The handover's divider, just before the lawyer's question it went with (handoff.ts finds it): a quiet centred line
// between hairlines, "Switched to ChatGPT 6.2 Sol" naming only what is known (UC-1512). It stays pending through the
// new agent's whole first answer (delivered only once that answer ends without an error), so pending reads the same
// quiet line with a small "Passing on" mark (UC-1406), never an alarm. A settled one that didn't reach the new agent for
// sure says so under the line in one plain sentence (handoff.ts, statusLine), by the host's own status: not confirmed,
// sent again by itself, nothing to do (UC-1514), quiet with an amber dot; not got, red with the failed mark: with the
// next message while that will carry it (UC-1515), else the next message goes as normal (UC-1516); a status from a
// newer Clauseo (UC-1517). The turn's own Try again is such a next message, so the divider has no button of its own.
// A handover too long to fit is no switch: "The conversation stayed with Claude." (UC-1513), quiet, nothing to open.
// A switch back (ContextHandoff.resumed: the earlier agent takes back its own session) reads "Back to Claude Opus 5.5,
// which picks up its own conversation" (UC-1510), and what was passed on starts "Only what Claude hadn't seen."
// (UC-1511); one that passes nothing has no "Passing on" mark and nothing to open.
// Behind "What was passed on" (UC-1401), what the new agent was given (UC-1402..1404). The model switch's divider
// (ModelSwitchDivider) shares its line.
import { useId, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconFailed } from "@clauseo/components/icons/icons.tsx";
import { useThreadWords } from "@clauseo/components/lib/words-thread.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import type { HandoffEntry, MessageId, StreamId, TurnId } from "../../transport/protocol.ts";
import {
  agentWord,
  endText,
  passesNothing,
  shownOf,
  statusLine,
  useHandoff,
  type Shown,
  type Standing,
} from "./handoff.ts";
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
      {entry.resumed === true && (
        <>
          <Say>
            {w.th("Only what {agent} hadn't seen.", { agent: agentWord(entry.to.agent) })}
          </Say>{" "}
        </>
      )}
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

/** The divider's own words: a switch, a switch back, or a handover too long, which moved nothing. */
function lineKey(entry: HandoffEntry, standing: Standing) {
  if (standing.tooLong) return "The conversation stayed with {agent}." as const;
  if (entry.resumed === true)
    return "Back to {agent}, which picks up its own conversation" as const;
  return "Switched to {agent}" as const;
}

/** Who the line names: the agent the conversation stayed with, else where it went, with its model when known. */
function lineAgent(entry: HandoffEntry, standing: Standing): string {
  return standing.tooLong ? agentWord(entry.from.agent) : endText(entry.to);
}

const TONE = { quiet: "text-quiet", amber: "text-quiet", red: "text-red-text" } as const;

/** The divider for one handoff, its status as shown (handoff.ts, shownOf) and where the thread stands for it. */
export function HandoffDivider({
  entry,
  shown,
  standing,
}: {
  entry: HandoffEntry;
  shown: Shown;
  standing: Standing;
}) {
  const w = useThreadWords();
  const [open, setOpen] = useState(false);
  const detail = useId();
  const line = statusLine(shown, agentWord(entry.to.agent), standing);
  const nothing = passesNothing(entry) || standing.tooLong;
  return (
    <div
      data-testid="handoff-divider"
      data-state={shown}
      data-resumed={entry.resumed === true ? "" : undefined}
      className="my-(--s4)"
    >
      <div className={`flex items-center gap-(--s3) text-small ${TONE[line.tone]}`}>
        <span aria-hidden className={LINE} />
        <span className="flex max-w-[80%] items-center gap-(--s2) text-center">
          {line.tone === "red" && <IconFailed size={14} />}
          {line.tone === "amber" && (
            <span
              aria-hidden
              data-testid="handoff-amber"
              className="size-1.5 rounded-full bg-yellow"
            />
          )}
          <span data-testid="handoff-line">
            <Say>{w.th(lineKey(entry, standing), { agent: lineAgent(entry, standing) })}</Say>
          </span>
          {shown === "pending" && !nothing && (
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
        {line.sentence !== null && (
          <span data-testid="handoff-status" className={`text-small ${TONE[line.tone]}`}>
            <Say>{w.th(line.sentence.key, line.sentence.slots)}</Say>
          </span>
        )}
        {!nothing && (
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
        )}
      </div>
      {open && <PassedOn entry={entry} id={detail} />}
    </div>
  );
}

/** The handoff that goes with the live question `message`, if any. */
export function LiveHandoff({ stream, message }: { stream: StreamId; message: MessageId }) {
  const found = useHandoff(stream, message);
  if (found === null) return null;
  return (
    <HandoffDivider
      entry={found.entry}
      shown={shownOf(found.entry.status)}
      standing={found.standing}
    />
  );
}

/** The model switch above the live question of `turn`, if its answer ran on another model or effort. */
export function LiveModelSwitch({ stream, turn }: { stream: StreamId; turn: TurnId }) {
  const change = useModelSwitch(stream, turn);
  return change === undefined ? null : <ModelSwitchDivider from={change.from} to={change.to} />;
}
