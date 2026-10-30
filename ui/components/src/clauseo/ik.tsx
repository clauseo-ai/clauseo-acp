// Indian Kanoon in the room. Its line, "powered by IKanoon", stands wherever its results
// or its text show. The research's results as the lawyer sees them, with each judgment's fragment and whether it has
// been read; the guard at 300 lookups, which waits for the lawyer's hand; and Indian Kanoon unavailable, while the
// judgments already read still open.
import { useId, useState } from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { CollapsiblePanel, CollapsibleTrigger } from "../ui/collapsible.tsx";
import { IconAccept, IconOpen, IconSearch } from "../icons/icons.tsx";
import { useReaderWords } from "../lib/words-reader.ts";
import { BenchMark } from "./citations.tsx";
import { fragmentRuns, type ReadAlready, type Result } from "./judgments.ts";
import { benchOf, keepIds } from "./matter.ts";
import { Dot, Say } from "./say.tsx";

/** Indian Kanoon's own line, as its terms ask, in its own words in both languages. */
export function PoweredByIK({ className }: { className?: string }) {
  return (
    <span className={className === undefined ? "ik-powered" : `ik-powered ${className}`} lang="en">
      powered by IKanoon
    </span>
  );
}

/** One result: the bench mark, the judgment's name, For us or They will cite; its report, court and date; the
 * fragment Indian Kanoon matched, its words marked; and Open, or, while Indian Kanoon is down, why it can't open yet.
 */
function ResultRow({
  r,
  offline,
  onOpen,
}: {
  r: Result;
  offline: boolean;
  onOpen: (id: string, from: HTMLElement) => void;
}) {
  const w = useReaderWords();
  const canOpen = r.read !== null || !offline;
  return (
    <li className="ik-result">
      <span className="ik-result-mark">
        <BenchMark bench={benchOf("Supreme Court", r.bench)} stance={r.stance} />
      </span>
      <p className="ik-result-name" lang="en">
        {r.short}
      </p>
      <Chip role={r.stance === "against" ? "cite" : "for"} className="ik-result-stance" />
      <p className="ik-result-where">
        <span lang="en" className="ik-result-cite">
          {keepIds(r.cite)}
        </span>
        <Dot />
        <span>
          <Say>{`${w.r("Supreme Court")}, ${r.decided}`}</Say>
        </span>
      </p>
      <blockquote className="ik-result-fragment" lang="en">
        {fragmentRuns(keepIds(r.fragment)).map((f, i) =>
          f.marked ? <mark key={i}>{f.text}</mark> : <span key={i}>{f.text}</span>,
        )}
      </blockquote>
      <div className="ik-result-foot">
        <span className="ik-result-read">
          {r.read !== null
            ? w.r("Read {time}", { time: w.clock(r.read) })
            : canOpen
              ? w.r("Not read yet")
              : w.r("Opens when Indian Kanoon answers")}
        </span>
        <Button
          kind="tone"
          size="small"
          disabled={!canOpen}
          aria-label={w.t("Open {name}", { name: r.name })}
          onClick={(e) => onOpen(r.id, e.currentTarget.closest("li") ?? e.currentTarget)}
        >
          {w.t("Open")}
          <IconOpen size={14} />
        </Button>
      </div>
    </li>
  );
}

/** The research's results, as the lawyer sees them after asking: the Supreme Court's judgments, then the High
 * Courts' folded under their count, and Indian Kanoon's line at the foot. */
export function ResearchResults({
  results,
  highCourts,
  offline,
  onOpen,
}: {
  /** The Supreme Court's judgments the research found. */
  results: readonly Result[];
  /** How many High Court judgments it found, folded under them. */
  highCourts: number;
  offline: boolean;
  onOpen: (id: string, from: HTMLElement) => void;
}) {
  const w = useReaderWords();
  const id = useId();
  const [open, setOpen] = useState(false);
  return (
    <section className="ik-results" aria-labelledby={`${id}-h`}>
      <header className="ik-results-head">
        <h3 className="ik-results-title" id={`${id}-h`}>
          <IconSearch size={16} className="ik-results-icon" />
          <Say>{w.r("Results from Indian Kanoon")}</Say>
        </h3>
        <span className="ik-results-count">
          <Say>{`${w.r("Supreme Court")}  ${results.length}`}</Say>
        </span>
      </header>
      <ol className="ik-result-list">
        {results.map((r) => (
          <ResultRow key={r.id} r={r} offline={offline} onOpen={onOpen} />
        ))}
      </ol>
      <BaseCollapsible.Root open={open} onOpenChange={setOpen}>
        <CollapsiblePanel>
          <p className="ik-results-more">
            <Say>{`${w.r("High Courts")}: ${w.r("{n} judgments", { n: highCourts })}`}</Say>
          </p>
        </CollapsiblePanel>
        <div className="ik-results-foot">
          <CollapsibleTrigger kind="more" className="ik-results-fold">
            {open ? w.r("Show fewer") : w.r("Show the {n} High Court judgments", { n: highCourts })}
          </CollapsibleTrigger>
          <PoweredByIK />
        </div>
      </BaseCollapsible.Root>
    </section>
  );
}

export type GuardState = "pending" | "continued" | "stopped";

/** The guard after 300 lookups: the question, what a lookup is, what has been found, and two answers. It waits for
 * the lawyer's hand, so its mark is court yellow. Answered, it folds to one grey line in the past tense. */
export function LookupGuard({
  state,
  at,
  so,
  expected,
  onAnswer,
}: {
  state: GuardState;
  /** When it was answered. */
  at: string;
  so: { searches: number; read: number; fit: number };
  onAnswer: (a: "continued" | "stopped") => void;
  /** Optional: when the research would finish if it goes on, said beside Continue. */
  expected?: string;
}) {
  const w = useReaderWords();
  const id = useId();
  if (state !== "pending") {
    return (
      <p className="perm-done ik-guard-done">
        <IconAccept size={14} className="perm-done-icon" />
        <span>
          <Say>
            {state === "continued"
              ? w.r("Continued past 300 lookups at {time}.", { time: w.clock(at) })
              : w.r("Stopped at 300 lookups at {time}. Clauseo answered with what was found.", {
                  time: w.clock(at),
                })}
          </Say>
        </span>
      </p>
    );
  }
  return (
    <div
      className="perm ik-guard"
      role="group"
      aria-labelledby={`${id}-ask`}
      aria-describedby={`${id}-why`}
    >
      <p className="perm-ask ik-guard-ask">
        <i className="ik-guard-dot" aria-hidden="true" />
        <span id={`${id}-ask`}>
          <Say>{w.r("This research has used 300 Indian Kanoon lookups. Continue?")}</Say>
        </span>
      </p>
      <p className="perm-kept" id={`${id}-why`}>
        <Say>
          {w.r(
            "A lookup is one search or one judgment read. Most research needs fewer than 300, so Clauseo asks before going on.",
          )}
        </Say>
      </p>
      <p className="perm-kept ik-guard-so">
        <Say>
          {w.r("So far {s} searches and {d} judgments read. {f} judgments fit.", {
            s: so.searches,
            d: so.read,
            f: so.fit,
          })}
        </Say>
      </p>
      <div className="perm-actions">
        <Button
          kind="tone"
          size="small"
          className="ik-guard-go"
          onClick={() => onAnswer("continued")}
        >
          {w.r("Continue")}
        </Button>
        <Button
          kind="quiet"
          size="small"
          className="ik-guard-stop"
          onClick={() => onAnswer("stopped")}
        >
          {w.r("Answer with what's found")}
        </Button>
        {/* what Continue costs, in time */}
        {expected !== undefined && (
          <span className="ik-guard-when">
            <Say>{w.r("Continuing, expected by {time}", { time: w.clock(expected) })}</Say>
          </span>
        )}
      </div>
    </div>
  );
}

/** Indian Kanoon unavailable: what happened and when Clauseo tries again, in clock time and in concrete, and the
 * judgments already read, which still open. */
export function IkUnavailable({
  retry,
  read,
  onOpen,
}: {
  retry: string;
  /** The judgments already read, which still open. */
  read: readonly ReadAlready[];
  onOpen: (id: string, from: HTMLElement) => void;
}) {
  const w = useReaderWords();
  const id = useId();
  return (
    <section className="ik-down" aria-labelledby={`${id}-h`}>
      <p className="ik-down-head">
        <IconSearch size={16} className="ik-down-icon" />
        <span id={`${id}-h`} className="ik-down-title">
          <Say>{w.r("Indian Kanoon is unavailable")}</Say>
        </span>
      </p>
      <p className="ik-down-body">
        <Say>
          {w.r("Searches wait until it answers. Clauseo tries again at {time}.", {
            time: w.clock(retry),
          })}
        </Say>
      </p>
      <p className="ik-down-sub">
        <Say>{w.r("Judgments already read still open.")}</Say>
      </p>
      <ul className="ik-down-list">
        {read.map((j) => (
          <li key={j.id} className="ik-down-row">
            <span className="ik-down-name" lang="en">
              {j.short}
            </span>
            <span className="ik-down-cite" lang="en">
              {keepIds(j.cite)}
            </span>
            <Button
              kind="quiet"
              size="small"
              className="ik-down-open"
              aria-label={w.t("Open {name}", { name: j.short })}
              onClick={(e) => onOpen(j.id, e.currentTarget.closest("li") ?? e.currentTarget)}
            >
              {w.t("Open")}
              <IconOpen size={14} />
            </Button>
          </li>
        ))}
      </ul>
      <p className="ik-down-foot">
        <PoweredByIK />
      </p>
    </section>
  );
}
