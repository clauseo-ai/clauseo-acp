// A helper's card in the lead's answer (H1 of the helpers design, round 24), where the step that
// launched it stood or, with none (ChatGPT), where its entry arrived (helper-view.ts leadRows). It
// says who the helper is and on what model, what it was asked (the agent's short label, else the
// task cut to about two lines; ChatGPT gives neither, and its name says what it does), then live
// the step in hand in the step group's own words with the pen, or once ended the start of what it
// found, why it failed, or what it did before it was stopped, as plain words read from its Markdown
// (helper-plain.ts). Failure is in the primary ink with the break, never red. Time in whole
// minutes, read again each minute while it works; no seconds tick. A press opens its steps and its
// words (through the answer's Markdown) under it, in place; H2 moves that to the right-hand pane. A
// helper it started (ChatGPT) has a small card of its own inside it, one level; anything deeper
// folds into that small card.
import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { Markdown } from "@clauseo/components/markdown/markdown.tsx";
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
import { helperOfEntryId, helperPartOf, type HelperEntry } from "./helper-entries.ts";
import { allPlain, firstPlain } from "./helper-plain.ts";
import {
  contentPick,
  cut,
  inlineOf,
  menuNamePick,
  modelWords,
  nameWords,
  reportOf,
  stateOf,
  stateWords,
  untilNextMinute,
  type HelperContent,
  type HelperState,
  unnamedPick,
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

/** The conversation's agent by its header: ChatGPT's or Claude's; another agent by its plan name.
 */
const agentKindPick = (v: StreamView | undefined): "codex" | "claude" | null => {
  const agent = v?.header?.header?.agent ?? "";
  return agent.startsWith("codex") ? "codex" : agent.startsWith("claude") ? "claude" : null;
};

function useLeadName(stream: StreamId): string {
  const plan = planNameOf(useAgentPlan());
  const kind = useStreamPart(useViewStore(), stream, agentKindPick);
  return kind === "codex" ? "ChatGPT" : kind === "claude" ? "Claude" : plan;
}

/** The card's last lines: the step in hand, or how it ended and what it did. */
function Outcome({
  h,
  state,
  items,
  lead,
}: {
  h: HelperEntry;
  state: HelperState;
  items: HelperContent["items"];
  lead: string;
}) {
  const { lang } = useDisplay();
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
  const { limited, report } = reportOf(inlineOf(h.summary));
  // cut at its turn limit: done, with the limit said plainly, never the adapter's bracket
  const limit = limited
    ? steps(lang, "It stopped at its turn limit before reporting back to {agent}.", { agent: lead })
    : "";
  const found =
    state === "done"
      ? limited
        ? report === ""
          ? limit
          : cut(`${limit} ${firstPlain(report)}`, FOUND_ON_CARD)
        : cut(firstPlain(report), FOUND_ON_CARD) || steps(lang, "Finished.")
      : state === "failed"
        ? report === ""
          ? steps(lang, "It ended without reporting back to {agent}.", { agent: lead })
          : cut(allPlain(report), FOUND_ON_CARD)
        : "";
  const after =
    did === ""
      ? ""
      : state === "done"
        ? did
        : steps(lang, state === "failed" ? "Before that: {what}" : "By then: {what}", {
            what: did,
          });
  if (found === "" && after === "") return null;
  return (
    <span className="helper-found text-small-row" data-testid="helper-found">
      {found !== "" && (
        <span className="helper-summary">
          {state === "failed" && <IconBreak size={14} className="helper-break" />}
          <bdi>{found}</bdi>
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

/** A helper's words, through the answer's own Markdown; `streaming` while they may still grow, so
 * a link in the last block stays inert until it is whole, as in the lead's answer. */
function Words({ text, streaming }: { text: string; streaming: boolean }) {
  if (text.trim() === "") return null;
  return (
    <div className="helper-words" dir="auto">
      <Markdown text={text} streaming={streaming} />
    </div>
  );
}

/** Its steps and words, opened in place: a step as an opened group's row, a run of its words (no
 * step between) as one Markdown text, the last run streaming while the helper works. A box of its
 * own height at most, scrolled within, so a long report never pushes the answer away. */
function Rows({
  id,
  items,
  working,
}: {
  id: string;
  items: HelperContent["items"];
  working: boolean;
}) {
  const { lang } = useDisplay();
  const out: ReactNode[] = [];
  let words: string[] = [];
  let wordsKey = "";
  const flush = (last: boolean) => {
    if (words.length > 0)
      out.push(
        <StepThought key={wordsKey}>
          <Words text={words.join("")} streaming={working && last} />
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
      flush(false);
      out.push(<CallRow key={key} id={key} call={e.tool_call} lang={lang} />);
    }
  }
  flush(true);
  return (
    // a scrolled box takes the keyboard's arrows once focused
    // oxlint-disable-next-line jsx-a11y/no-noninteractive-tabindex
    <ol id={id} className="step-group-rows helper-rows" data-testid="helper-rows" tabIndex={0}>
      {out}
    </ol>
  );
}

/** One helper's card, by its entry id, live or from an older page. `child`: the small card of a
 * helper a helper started, inside its parent's. */
export function HelperCard({
  id,
  stream,
  paged,
  child = false,
}: {
  id: EntryId;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  child?: boolean;
}) {
  const helper = helperOfEntryId(id);
  const store = useViewStore();
  const pick = useMemo(() => contentPick(helper, paged, child), [helper, paged, child]);
  const content = useStreamPart(store, stream, pick);
  const countPick = useMemo(() => unnamedPick(helper, paged), [helper, paged]);
  const n = useStreamPart(store, stream, countPick);
  const model = content.helper?.model;
  const namePick = useMemo(() => menuNamePick(model), [model]);
  const menuName = useStreamPart(store, stream, namePick);
  const lead = useLeadName(stream);
  const h = content.helper;
  if (h === undefined) return null;
  return (
    <Card
      id={id}
      h={h}
      content={content}
      n={n}
      model={menuName ?? modelWords(h.model)}
      lead={lead}
      kids={
        content.children.length === 0 ? null : (
          <div className="helper-kids" data-testid="helper-kids">
            {content.children.map((c) => (
              <HelperCard key={c} id={c} stream={stream} paged={paged} child />
            ))}
          </div>
        )
      }
      child={child}
    />
  );
}

function Card({
  id,
  h,
  content,
  n,
  model,
  lead,
  kids,
  child,
}: {
  id: EntryId;
  h: HelperEntry;
  content: HelperContent;
  n: number;
  /** The model in the menu's words, or null when not known. */
  model: string | null;
  /** The lead agent's name: Claude or ChatGPT. */
  lead: string;
  kids: ReactNode;
  child: boolean;
}) {
  const { lang } = useDisplay();
  const [open, setOpen] = useState(false);
  const rows = useId();
  const { items, turnEnd } = content;
  const state = stateOf(h);
  const now = useMinuteClock(h.started_at, state === "working" && turnEnd === undefined);
  const task = h.label ?? (inlineOf(h.task) === null ? null : cut(inlineOf(h.task)!, TASK_ON_CARD));
  const time = stateWords(lang, h, now, turnEnd);
  return (
    <div
      className={child ? "helper-card helper-card-child" : "helper-card"}
      data-testid={child ? "helper-child" : "helper-card"}
      data-entry-id={id}
      data-helper={h.helper}
      data-state={state}
      data-background={state === "working" && turnEnd !== undefined ? "" : undefined}
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
        <Outcome h={h} state={state} items={items} lead={lead} />
      </button>
      {kids}
      {open && items.length > 0 && <Rows id={rows} items={items} working={state === "working"} />}
    </div>
  );
}

/** Words of a helper whose entry the window doesn't have: flat in the answer, through the answer's
 * Markdown, never dropped. */
export function HelperWordsFlat({ id, entry }: { id: EntryId; entry: ViewEntry }) {
  const part = helperPartOf(entry);
  if (part?.kind !== "text" || part.entry.text.trim() === "") return null;
  return (
    <div
      data-testid="helper-text-flat"
      data-entry-id={id}
      className="not-first:mt-(--s3) text-small text-quiet"
      dir="auto"
    >
      <Markdown text={part.entry.text} />
    </div>
  );
}
