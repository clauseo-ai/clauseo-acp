// The papers as they stood. A draft on its sheet, as the turned page sets it, at any version, or as a blackline
// against another; a list of dates the same way; a PDF only changed or unchanged. The lawyer's own words stand at 600
// wherever they are. Switching versions keeps the same paragraph where it was in view (version-anchor.ts).
import type { ReactNode, RefObject } from "react";
import { Button } from "../ui/button.tsx";
import { IconOpen, IconPaperBookPage } from "../icons/icons.tsx";
import { useVersionWords } from "../lib/words-versions.ts";
import { Heading } from "./heading.tsx";
import { keepIds } from "./matter.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";
import type { ParaDiff, RowDiff, Run } from "./versions-diff.ts";
import { useStuck } from "./stuck.ts";

/** Words as runs: the same, came in, or went out; the lawyer's own at 600. */
export function Runs({ runs }: { runs: readonly Run[] }) {
  return (
    <>
      {runs.map((r, i) => {
        const words = <Say>{keepIds(r.text)}</Say>;
        const inner = r.mine ? <b className="ver-mine">{words}</b> : words;
        if (r.kind === "ins")
          return (
            <ins key={i} className="ver-ins">
              {inner}
            </ins>
          );
        if (r.kind === "del")
          return (
            <del key={i} className="ver-del">
              {inner}
            </del>
          );
        return <span key={i}>{inner}</span>;
      })}
    </>
  );
}

/** The head of a pleading's sheet, as filed. */
export type SheetHeading = {
  court: string;
  caseLine: string;
  parties: readonly { name: string; role: string }[];
  title: string;
};

/** A pleading on its sheet: the court, the case line, the parties, the title and the paragraphs. marked: the
 * paragraphs a blackline changed carry a bar in the margin. */
export function DraftSheet({
  head,
  diffs,
  marked,
  differs,
}: {
  head: SheetHeading;
  diffs: readonly ParaDiff[];
  marked: boolean;
  /** Looking back without marks: the paragraphs that read differently now, which the sheet opens at. */
  differs?: ReadonlySet<number> | undefined;
}) {
  const w = useVersionWords();
  const { id } = useRoom();
  return (
    <article className="page ver-page" aria-labelledby={id("ver-title")}>
      <p className="page-court">
        <Say>{head.court}</Say>
      </p>
      <p className="page-case">
        <Say>{head.caseLine}</Say>
      </p>
      <div className="page-parties">
        {head.parties.map((p, i) => (
          <div key={p.name} className="contents">
            {i > 0 && <p className="page-versus">{w.t("Versus")}</p>}
            <p className="page-party">
              <span>
                <Say>{p.name}</Say>
              </span>
              <span className="page-role">{p.role}</span>
            </p>
          </div>
        ))}
      </div>
      <Heading level={2} className="page-title" id={id("ver-title")}>
        <Say>{head.title}</Say>
      </Heading>
      <div className="page-paras">
        {diffs.map((d) => (
          <p
            key={d.place}
            className="ver-place"
            data-place={d.place}
            data-change={marked && d.kind !== "same" ? d.kind : undefined}
            data-differs={differs?.has(d.place) === true ? "" : undefined}
          >
            <Runs runs={d.runs} />
          </p>
        ))}
      </div>
      <p className="page-foot">
        <span aria-hidden="true">1</span>
        <span className="sr-only">{w.t("Page 1")}</span>
      </p>
    </article>
  );
}

/** A list of dates on its sheet, a row to each date, under its title as filed. */
export function DatesSheet({
  title,
  rows,
  marked,
}: {
  title: string;
  rows: readonly RowDiff[];
  marked: boolean;
}) {
  const { id } = useRoom();
  return (
    <article className="page ver-page" aria-labelledby={id("ver-dates")}>
      <Heading level={2} className="page-title ver-dates-title" id={id("ver-dates")}>
        <Say>{title}</Say>
      </Heading>
      <dl className="ver-dates">
        {rows.map((r) => (
          <div
            key={r.row}
            className="ver-date-row ver-place"
            data-place={r.row}
            data-change={marked && r.kind !== "same" ? r.kind : undefined}
          >
            <dt>
              <Runs runs={r.date} />
            </dt>
            <dd>
              <Runs runs={r.event} />
            </dd>
          </div>
        ))}
      </dl>
    </article>
  );
}

/** One side of a PDF's comparison: when, and what the computer can say of the file then. */
export type PdfSide = { head: string; facts: string; saved: string };

/** A PDF says only whether it changed: its name, Changed or Unchanged since then, what the file was then and is now,
 * and a way to open each, since its words can't be set side by side. */
export function PdfSheet({
  title,
  changed,
  state,
  then,
  now,
  onOpen,
}: {
  title: string;
  changed: boolean;
  /** "Changed since 3:42 pm on 14 October". */
  state: string;
  then: PdfSide;
  now: PdfSide;
  /** Opens the file as it was, or as it is now. */
  onOpen?: (which: "then" | "now") => void;
}) {
  const w = useVersionWords();
  const side = (s: PdfSide, which: "then" | "now") => (
    <div className="ver-pdf-side" data-side={which}>
      <p className="ver-pdf-when">
        <Say>{s.head}</Say>
      </p>
      <p className="ver-pdf-facts">
        <Say>{s.facts}</Say>
      </p>
      <p className="ver-pdf-saved">
        <Say>{s.saved}</Say>
      </p>
      <Button kind="tone" size="small" className="ver-pdf-open" onClick={() => onOpen?.(which)}>
        <IconOpen size={16} />
        {which === "then" ? w.v("Open as it was") : w.v("Open now")}
      </Button>
    </div>
  );
  return (
    <article className="ver-pdf" aria-label={title}>
      <header className="ver-pdf-head">
        <span className="card-icon ver-pdf-icon">
          <IconPaperBookPage size={18} />
        </span>
        <span className="ver-pdf-name">
          <span className="ver-pdf-title">
            <Say>{title}</Say>
          </span>
          <span className="ver-pdf-state" data-changed={changed ? "" : undefined}>
            <Say>{state}</Say>
          </span>
        </span>
      </header>
      <div className="ver-pdf-pair">
        {side(then, "then")}
        {changed && side(now, "now")}
      </div>
      <p className="ver-pdf-note">
        <Say>{w.v("A PDF shows only whether it changed. Open each to read it.")}</Say>
      </p>
    </article>
  );
}

/** Over the sheet: which paper this is and when, and a way to keep a copy of it as it stood. */
export function SheetHead({ label, action }: { label: string; action?: ReactNode }) {
  return (
    <div className="ver-sheet-head">
      <p className="ver-sheet-label">
        <Say>{label}</Say>
      </p>
      {action}
    </div>
  );
}

/** A paper that wasn't in the matter yet at this version: a bare sheet that says so, and when it was first made. */
export function MissingSheet({ title, first }: { title: string; first: string }) {
  const w = useVersionWords();
  return (
    <div className="ver-missing">
      <p className="ver-missing-title">
        <Say>{w.v("{title} did not exist yet.", { title })}</Say>
      </p>
      <p className="ver-missing-note">
        <Say>{first}</Say>
      </p>
    </div>
  );
}

/** The desk the sheet lies on, scrolling on its own, focusable so it scrolls from the keyboard. */
export function Desk({
  label,
  scrollRef,
  children,
  className,
}: {
  label: string;
  scrollRef: RefObject<HTMLDivElement | null>;
  children: ReactNode;
  className?: string | undefined;
}) {
  // the sheet's heading casts the pinned edge once the sheet scrolls under it
  useStuck(scrollRef, ".ver-sheet-head", label);
  return (
    <div
      className={className === undefined ? "ver-desk" : `ver-desk ${className}`}
      ref={scrollRef}
      tabIndex={0}
      role="region"
      aria-label={label}
    >
      {children}
    </div>
  );
}
