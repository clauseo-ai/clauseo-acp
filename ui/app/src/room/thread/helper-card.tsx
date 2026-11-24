// A helper's card in the lead's answer (H1 of the helpers design, round 24), where the step that
// launched it stood or, with none (ChatGPT), where its entry arrived (helper-view.ts leadRows). It
// says who the helper is and on what model, what it was asked (the agent's short label, else the
// task cut to about two lines; ChatGPT gives neither, and its name says what it does), then live
// the step in hand in the step group's own words with the pen, or once ended the start of what it
// found, why it failed, or what it did before it was stopped. Failure is in the primary ink with
// the break, never red. Time in whole minutes, read again each minute while it works; no seconds
// tick. A press opens its steps and words under it, in place, as an opened step group's rows; H2
// moves that to the right-hand pane.
import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { PenMark } from "@clauseo/components/clauseo/pen-mark.tsx";
import { StepThought } from "@clauseo/components/clauseo/step-group.tsx";
import { ToolKindIcon } from "@clauseo/components/clauseo/tool-kind-icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { IconBreak, IconChecking, IconStop, IconTick } from "@clauseo/components/icons/icons.tsx";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { useAgentPlan } from "./agent-plan.ts";
import { helperPartOf, type HelperEntry, type HelperId } from "./helper-entries.ts";
import {
  contentPick,
  cut,
  helperIds,
  inlineOf,
  menuNamePick,
  modelWords,
  named,
  nameWords,
  stateOf,
  stateWords,
  untilNextMinute,
  type HelperContent,
  type HelperState,
} from "./helper-view.ts";
import { planNameOf } from "./limit-words.ts";
import { CallRow } from "./step-group-parts.tsx";
import { lineOf } from "./steps.ts";
import "./helper-card.css";

/** The most of a task the card shows when the agent gave no label: about two lines. */
const TASK_ON_CARD = 160;
/** The most of what it found the card shows: about three lines. */
const FOUND_ON_CARD = 280;

/** The state's glyph: no spinner, no colour. */
function StateGlyph({ state }: { state: HelperState }) {
  const p = { size: 14 as const, className: "helper-glyph" };
  if (state === "working") return <IconChecking {...p} />;
  if (state === "done") return <IconTick {...p} />;
  if (state === "failed") return <IconBreak {...p} />;
  return <IconStop {...p} />;
}

/** The clock the card reads: now, read again as each minute since `started` turns over, while
 * `live`. */
function useMinuteClock(started: number, live: boolean): number {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!live) return;
    const t = setTimeout(() => setNow(Date.now()), untilNextMinute(started, now));
    return () => clearTimeout(t);
  }, [started, live, now]);
  return now;
}

/** The first paragraph of a report, its markdown marks off, cut for the card. */
const firstPart = (text: string) =>
  cut(text.split(/\n\s*\n/)[0]!.replace(/[#*_`>]/g, ""), FOUND_ON_CARD);

/** The card's last lines: the step in hand, or how it ended and what it did. */
function Outcome({ h, items }: { h: HelperEntry; items: HelperContent["items"] }) {
  const { lang } = useDisplay();
  const agent = planNameOf(useAgentPlan());
  const state = stateOf(h);
  const entries = useMemo(() => items.map((x) => x[1]), [items]);
  if (state === "working") {
    const line = lineOf(lang, entries, true);
    const said = line.line !== "" ? line.line : stepLive(lang, "Working on it.");
    return (
      <span className="helper-now text-small-row" data-testid="helper-now">
        {line.kinds[0] !== undefined && (
          <ToolKindIcon kind={line.kinds[0]} size={14} className="step-icon" />
        )}
        <span>
          <Say>{said}</Say>
          {line.more !== "" && <span className="helper-more"> {line.more}</span>}
          <PenMark className="helper-pen" />
        </span>
      </span>
    );
  }
  const did = lineOf(lang, entries, false).line;
  const summary = inlineOf(h.summary);
  const lead =
    state === "done"
      ? (summary === null ? "" : firstPart(summary)) || steps(lang, "Finished.")
      : state === "failed"
        ? summary === null || summary.trim() === ""
          ? steps(lang, "It ended without reporting back to {agent}.", { agent })
          : cut(summary, FOUND_ON_CARD)
        : "";
  const after =
    did === ""
      ? ""
      : state === "done"
        ? did
        : steps(lang, state === "failed" ? "Before that: {what}" : "By then: {what}", {
            what: did,
          });
  return (
    <span className="helper-found text-small-row" data-testid="helper-found">
      {lead !== "" && (
        <span className="helper-summary">
          {state === "failed" && <IconBreak size={14} className="helper-break" />}
          <bdi>{lead}</bdi>
        </span>
      )}
      {after !== "" && (
        <span className="helper-did">
          <Say>{after}</Say>
        </span>
      )}
    </span>
  );
}

/** Its steps and words, opened in place: a step as an opened group's row, a run of its words as one
 * paragraph. */
function Rows({ id, items }: { id: string; items: HelperContent["items"] }) {
  const { lang } = useDisplay();
  const out: ReactNode[] = [];
  let words: string[] = [];
  let wordsKey = "";
  const flush = () => {
    if (words.length > 0)
      out.push(
        <StepThought key={wordsKey}>
          <p dir="auto" className="helper-words">
            {words.join("")}
          </p>
        </StepThought>,
      );
    words = [];
  };
  for (const [key, e] of items) {
    const part = helperPartOf(e);
    if (part?.kind === "text") {
      if (words.length === 0) wordsKey = key;
      words.push(part.entry.text);
    } else if (e.tool_call !== undefined) {
      flush();
      out.push(<CallRow key={key} id={key} call={e.tool_call} lang={lang} />);
    }
  }
  flush();
  return (
    <ol id={id} className="step-group-rows helper-rows" data-testid="helper-rows">
      {out}
    </ol>
  );
}

/** The helper's place among the conversation's helpers without a name, from 1: "Helper 2". */
function unnamedPick(helper: HelperId, paged: ReadonlyMap<EntryId, ViewEntry>) {
  return (v: StreamView | undefined): number => {
    let n = 0;
    for (const id of [...paged.keys(), ...helperIds(v?.ids ?? [])]) {
      const part = helperPartOf(paged.get(id) ?? v?.entries.get(id));
      if (part?.kind !== "helper" || named(part.entry.name)) continue;
      n += 1;
      if (part.entry.helper === helper) return n;
    }
    return Math.max(1, n);
  };
}

/** One helper's card, by its entry id (`helper:<id>`), live or from an older page. */
export function HelperCard({
  id,
  stream,
  paged,
}: {
  id: EntryId;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
}) {
  const helper = id.slice("helper:".length);
  const store = useViewStore();
  const pick = useMemo(() => contentPick(helper, paged), [helper, paged]);
  const { helper: h, items } = useStreamPart(store, stream, pick);
  const countPick = useMemo(() => unnamedPick(helper, paged), [helper, paged]);
  const n = useStreamPart(store, stream, countPick);
  const model = h?.model;
  const namePick = useMemo(() => menuNamePick(model), [model]);
  const menuName = useStreamPart(store, stream, namePick);
  if (h === undefined) return null;
  return <Card id={id} h={h} items={items} n={n} model={menuName ?? modelWords(h.model)} />;
}

function Card({
  id,
  h,
  items,
  n,
  model,
}: {
  id: EntryId;
  h: HelperEntry;
  items: HelperContent["items"];
  n: number;
  /** The model in the menu's words, or null when not known. */
  model: string | null;
}) {
  const { lang } = useDisplay();
  const [open, setOpen] = useState(false);
  const rows = useId();
  const state = stateOf(h);
  const now = useMinuteClock(h.started_at, state === "working");
  const task = h.label ?? (inlineOf(h.task) === null ? null : cut(inlineOf(h.task)!, TASK_ON_CARD));
  const time = stateWords(lang, h, now);
  return (
    <div
      className="helper-card"
      data-testid="helper-card"
      data-entry-id={id}
      data-helper={h.helper}
      data-state={state}
    >
      <button
        type="button"
        className="helper-open"
        aria-expanded={open}
        aria-controls={rows}
        disabled={items.length === 0}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="helper-head text-small font-ui">
          <StateGlyph state={state} />
          <span className="helper-who">
            <b className="helper-name" data-testid="helper-name">
              {nameWords(lang, h.name, n)}
            </b>
            {model !== null && <span className="helper-model">{model}</span>}
          </span>
          {time !== "" && (
            <span className="helper-state" data-testid="helper-state">
              <Say>{time}</Say>
            </span>
          )}
        </span>
        {task !== null && task !== "" && (
          <span className="helper-task text-small-row" data-testid="helper-task">
            <bdi>{task}</bdi>
          </span>
        )}
        <Outcome h={h} items={items} />
      </button>
      {open && items.length > 0 && <Rows id={rows} items={items} />}
    </div>
  );
}

/** Words of a helper whose entry the window doesn't have: flat in the answer, as the lead's quiet
 * rows, never dropped. */
export function HelperWordsFlat({ id, entry }: { id: EntryId; entry: ViewEntry }) {
  const part = helperPartOf(entry);
  if (part?.kind !== "text") return null;
  return (
    <p
      data-testid="helper-text-flat"
      data-entry-id={id}
      className="not-first:mt-(--s3) text-small text-quiet"
      dir="auto"
    >
      {part.entry.text}
    </p>
  );
}
