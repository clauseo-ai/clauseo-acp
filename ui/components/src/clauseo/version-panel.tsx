// The timeline. A Versions panel stands where the rail of matters stood and lists every version of the matter across
// its conversations, newest first, grouped by day: the time, who made it and the papers it touched. It still reads
// with hundreds of versions: each day heading stays put while its versions scroll under it, a long run of small
// versions by one hand folds behind its latest, days before the last few fold under one line, and a switch shows only
// Clauseo's versions or only the lawyer's. Choosing one shows the whole room as it stood. The newest is now. The
// arrow keys move along what shows. Its foot says where the history is kept.
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { SegmentedSwitch } from "../ui/segmented.tsx";
import { IconFold } from "../icons/icons.tsx";
import { useVersionWords } from "../lib/words-versions.ts";
import { cn } from "../lib/cn.ts";
import { Heading } from "./heading.tsx";
import { useRoom } from "./room-scope.ts";
import { useStuck } from "./stuck.ts";
import { Say } from "./say.tsx";
import {
  dayKey,
  foldedSpan,
  groupsOf,
  OPEN_DAYS,
  papersOf,
  planOf,
  timeOf,
  whoOf,
  type Calendar,
  type Item,
  type NameOf,
  type Row,
  type Show,
  type Version,
} from "./versions-model.ts";

/** A version that couldn't be kept, shown at the top of the timeline where it would have stood. */
export type Unsaved = { at: string; who: string; why: string };

/** A clock time holds together on its line: "4:28 pm" never breaks before "pm". */
const hold = (time: string) => time.replace(/ (am|pm)$/, "\u00a0$1");

export function VersionPanel({
  history,
  calendar,
  name,
  at,
  onPick,
  onClose,
  unsaved = null,
  where,
}: {
  history: readonly Version[];
  /** Today, and the words for a day. */
  calendar: Calendar;
  /** A paper's name, as its tile titles it, by id. */
  name: NameOf;
  /** The version in view, by its place in history; the last is now. */
  at: number;
  onPick: (i: number) => void;
  onClose: () => void;
  /** A version that couldn't be saved, at the top. */
  unsaved?: Unsaved | null;
  /** The foot: where the history is kept, as a control that says more. */
  where?: ReactNode;
}) {
  const w = useVersionWords();
  const { id } = useRoom();
  const list = useRef<HTMLDivElement>(null);
  const last = history.length - 1;
  const [show, setShow] = useState<Show>("all");
  const [openRuns, setOpenRuns] = useState<ReadonlySet<string>>(new Set());
  const [older, setOlder] = useState(false);
  const groups = groupsOf(history, show, w.lang, calendar);
  const current = history[at];
  // what's in view always shows: its day, and its run
  const atGroup = groups.findIndex((g) => current !== undefined && g.key === dayKey(current));
  const olderOpen = older || atGroup >= OPEN_DAYS;
  const runOpen = (r: Extract<Row, { kind: "run" }>) =>
    openRuns.has(r.key) || r.rest.some((x) => x.i === at);
  const toggleRun = (key: string) =>
    setOpenRuns((s) => {
      const next = new Set(s);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  // the version in view stands in view in the list too, when it's chosen from elsewhere or opens the page
  useLayoutEffect(() => {
    const box = list.current;
    const b = box?.querySelector<HTMLElement>(`.ver-entry[data-at="${at}"]`);
    if (box == null || b == null) return;
    const r = b.getBoundingClientRect();
    const o = box.getBoundingClientRect();
    // clear of the day's heading, which holds the top
    const head = 40;
    if (r.top < o.top + head || r.bottom > o.bottom) box.scrollTop += r.top - o.top - head - 8;
  }, [at, olderOpen]);

  // each day's heading casts the pinned edge while its versions pass under it
  useStuck(list, ".ver-day", `${show}|${olderOpen ? 1 : 0}|${openRuns.size}|${history.length}`);

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    const all = [...(list.current?.querySelectorAll<HTMLButtonElement>(".ver-entry") ?? [])];
    if (all.length === 0) return;
    e.preventDefault();
    const here = all.findIndex((b) => b.dataset["at"] === String(at));
    const to =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? all.length - 1
          : Math.max(0, Math.min(all.length - 1, here + (e.key === "ArrowDown" ? 1 : -1)));
    const b = all[to];
    if (b === undefined) return;
    onPick(Number(b.dataset["at"]));
    b.focus();
  };

  const entry = ({ v, i }: Item) => {
    const papers = papersOf(w.lang, v, name);
    const plan = planOf(v);
    return (
      <li key={v.id}>
        <button
          type="button"
          className="ver-entry"
          data-at={i}
          aria-current={i === at ? "true" : undefined}
          tabIndex={i === at ? 0 : -1}
          title={i === last ? timeOf(w.lang, v) : undefined}
          onClick={() => onPick(i)}
        >
          {/* now says Now, and its clock time stands in the tooltip, never twice on the row */}
          <span className="ver-entry-time">{i === last ? w.v("Now") : timeOf(w.lang, v)}</span>
          <span className="ver-entry-who">
            <Say>{whoOf(w.lang, calendar, v, history)}</Say>
            {/* the plan, muted and last, held to the word before it so a wrap takes both: the one place it is the subject */}
            {plan !== null && (
              <span className="ver-entry-plan" lang="en">
                {"\u00a0·\u00a0"}
                {plan}
              </span>
            )}
          </span>
          {papers !== "" && (
            <span className="ver-entry-papers">
              <Say>{papers}</Say>
            </span>
          )}
          {v.large !== undefined && (
            <span className="ver-entry-note">
              <Say>{w.v("{file} is too large to keep versions", { file: v.large.file })}</Say>
            </span>
          )}
        </button>
      </li>
    );
  };

  const shown = olderOpen ? groups : groups.slice(0, OPEN_DAYS);
  const folded = groups.slice(OPEN_DAYS);
  const foldedCount = folded.reduce((n, g) => n + g.count, 0);
  const span = foldedSpan(w.lang, calendar, groups);

  return (
    <nav className="ver-panel" aria-labelledby={id("ver-panel-head")}>
      <header className="ver-panel-head">
        <Heading level={2} className="wall-title" id={id("ver-panel-head")}>
          {w.v("Versions")} <span className="wall-count">{history.length}</span>
        </Heading>
        <Button kind="quiet" size="small" className="ver-panel-close" onClick={onClose}>
          {w.ui("Close")}
        </Button>
      </header>
      <SegmentedSwitch<Show>
        label={w.v("Show versions by")}
        value={show}
        onValueChange={setShow}
        className="ver-show"
        options={[
          { value: "all", label: w.v("All") },
          { value: "clauseo", label: w.v("By Clauseo") },
          { value: "you", label: w.v("By you") },
        ]}
      />
      <div className="ver-panel-list" ref={list} onKeyDown={onKey}>
        {unsaved !== null && (
          <section className="ver-group">
            <ul className="ver-entries">
              <li className="ver-unsaved">
                <span className="ver-entry-time">{w.clock(unsaved.at)}</span>
                <span className="ver-entry-who">
                  <Say>{unsaved.who}</Say>
                </span>
                <span className="ver-entry-note ver-unsaved-why">
                  <Say>{unsaved.why}</Say>
                </span>
              </li>
            </ul>
          </section>
        )}
        {shown.map((g, gi) => (
          <section key={g.key} className={cn("ver-group", gi >= OPEN_DAYS && "ver-group-older")}>
            <Heading level={3} className="ver-day">
              <span>{g.head}</span>
              <span className="ver-day-count">
                {g.count === 1 ? w.v("1 version") : w.v("{n} versions", { n: g.count })}
              </span>
            </Heading>
            <ul className="ver-entries">
              {g.rows.map((r) => {
                if (r.kind === "one") return entry(r.item);
                const open = runOpen(r);
                const oldest = r.rest[r.rest.length - 1]?.v;
                const newest = r.rest[0]?.v;
                return (
                  <li key={r.key} className="ver-run">
                    <ul className="ver-entries">{entry(r.head)}</ul>
                    <button
                      type="button"
                      className="ver-run-fold"
                      aria-expanded={open}
                      onClick={() => toggleRun(r.key)}
                    >
                      <IconFold size={13} className="ver-run-icon" />
                      <span>
                        <Say>
                          {oldest === undefined || newest === undefined
                            ? ""
                            : w.v("{n} more · {from} to {to}", {
                                n: r.rest.length,
                                from: hold(timeOf(w.lang, oldest)),
                                to: hold(timeOf(w.lang, newest)),
                              })}
                        </Say>
                      </span>
                    </button>
                    {open && <ul className="ver-entries ver-run-rest">{r.rest.map(entry)}</ul>}
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
        {folded.length > 0 && !olderOpen && (
          <button
            type="button"
            className="ver-older"
            aria-expanded={false}
            onClick={() => setOlder(true)}
          >
            <IconFold size={13} className="ver-run-icon" />
            <span className="ver-older-words">
              <span className="ver-older-head">
                <Say>{w.v("Earlier, {span}", { span })}</Say>
              </span>
              <span className="ver-day-count">{w.v("{n} versions", { n: foldedCount })}</span>
            </span>
          </button>
        )}
      </div>
      {where}
    </nav>
  );
}
