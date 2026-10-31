// The judgment reader. A judgment the answer cites opens over the thread, from its row in
// the margin or from its excerpt card, and grows out of whatever it was opened from. Its text stands on a sheet with
// each paragraph's number in the margin, and it opens at the paragraph the answer quotes, set apart on the well with
// the words the answer relies on marked; once that paragraph scrolls away it pins to the top, as the question does.
// "Back to the answer" sends it back, and the thread underneath is exactly where it was left. The same frame holds an
// entry of the CCI and NCLAT index, which is an index and not a record.
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { Button } from "../ui/button.tsx";
import { Chip } from "../ui/chip.tsx";
import { Kbd } from "../ui/kbd.tsx";
import { IconBackToAnswer } from "../icons/icons.tsx";
import { motionAllowed } from "../lib/display.ts";
import { useReaderWords } from "../lib/words-reader.ts";
import { BenchMark } from "./citations.tsx";
import { markRuns, type IndexEntry, type Judgment } from "./judgments.ts";
import { benchOf, keepIds } from "./matter.ts";
import { dur, ease } from "./motion.ts";
import { useRoom } from "./room-scope.ts";
import { Say } from "./say.tsx";

/** The frame every reading opens in: a bar with Back at the left and what is open at the right, and the desk the
 * sheet lies on. It grows out of the rectangle it was opened from and shrinks back the way it came. */
function ReaderFrame({
  open,
  from,
  label,
  what,
  backRef,
  deskRef,
  onBack,
  pinned,
  children,
}: {
  open: boolean;
  /** Where it was opened from, in the window: a citation row, a card, a result. */
  from: DOMRect | null;
  label: string;
  what: ReactNode;
  backRef: RefObject<HTMLButtonElement | null>;
  deskRef: RefObject<HTMLDivElement | null>;
  onBack: () => void;
  pinned?: ReactNode;
  children: ReactNode;
}) {
  const w = useReaderWords();
  const section = useRef<HTMLElement>(null);
  // the reader stays on the page while it shrinks back; a timer, never the animation, takes it off
  const [shown, setShown] = useState(open);
  const [leaving, setLeaving] = useState(false);
  if (open && !shown) setShown(true);
  if (open && leaving) setLeaving(false);
  if (!open && shown && !leaving) setLeaving(true);

  const clipOf = (s: HTMLElement, r: DOMRect | null) => {
    if (r === null) return null;
    const b = s.getBoundingClientRect();
    const px = (v: number) => `${Math.max(0, Math.round(v))}px`;
    return `inset(${px(r.top - b.top)} ${px(b.right - r.right)} ${px(b.bottom - r.bottom)} ${px(r.left - b.left)} round 12px)`;
  };

  // opening: the frame grows out of what it was opened from on the spring, and its words come in once it is mostly
  // grown
  useLayoutEffect(() => {
    const s = section.current;
    if (!open || s === null || !motionAllowed()) return;
    const clip = clipOf(s, from);
    const grow = dur("turn");
    if (clip !== null) {
      s.animate([{ clipPath: clip }, { clipPath: "inset(0px 0px 0px 0px round 18px)" }], {
        duration: grow,
        easing: ease("spring"),
      });
    } else {
      s.animate(
        [
          { opacity: 0, transform: "translateY(8px)" },
          { opacity: 1, transform: "none" },
        ],
        { duration: dur("arrive"), easing: ease("arrive") },
      );
    }
    for (const part of s.children) {
      part.animate([{ opacity: 0 }, { opacity: 1 }], {
        duration: dur("state"),
        delay: Math.round(grow * 0.45),
        easing: "linear",
        fill: "backwards",
      });
    }
  }, [open, from]);

  // closing: its words go first, then it shrinks back into what it came from, faster than it grew
  useLayoutEffect(() => {
    if (!leaving) return;
    const s = section.current;
    let ms = 0;
    if (s !== null && motionAllowed()) {
      ms = dur("turn-back");
      const clip = clipOf(s, from);
      for (const part of s.children)
        part.animate([{ opacity: 1 }, { opacity: 0 }], {
          duration: dur("leave"),
          fill: "forwards",
        });
      s.animate(
        clip === null
          ? [{ opacity: 1 }, { opacity: 0 }]
          : [{ clipPath: "inset(0px 0px 0px 0px round 18px)" }, { clipPath: clip }],
        { duration: ms, easing: ease("leave"), fill: "forwards" },
      );
    }
    const t = window.setTimeout(() => {
      setShown(false);
      setLeaving(false);
    }, ms);
    return () => window.clearTimeout(t);
  }, [leaving, from]);

  if (!shown) return null;
  return (
    <section
      className="rd-reader"
      ref={section}
      role="group"
      aria-label={label}
      data-leaving={leaving ? "" : undefined}
    >
      <div className="rd-bar">
        <Button
          ref={backRef}
          kind="quiet"
          size="small"
          className="pr-s3 pl-s2"
          aria-keyshortcuts="ArrowLeft"
          onClick={onBack}
        >
          <IconBackToAnswer size={16} />
          {w.t("Back to the answer")}
          <Kbd>←</Kbd>
        </Button>
        <div className="rd-what">{what}</div>
      </div>
      <div className="rd-desk-wrap">
        {pinned}
        {/* focusable, so the judgment scrolls from the keyboard too */}
        <div className="rd-desk" ref={deskRef} tabIndex={0} role="region" aria-label={label}>
          {children}
        </div>
      </div>
    </section>
  );
}

/** A judgment's sheet: the court in capitals, its reports, the parties with their leaders, the bench and date, and
 * the paragraphs with their numbers in the margin. */
function JudgmentSheet({
  j,
  quotedRef,
}: {
  j: Judgment;
  quotedRef: RefObject<HTMLDivElement | null>;
}) {
  const w = useReaderWords();
  const { id } = useRoom();
  const bench = benchOf("court", j.bench);
  const reports = [j.reports.scc, j.reports.air, j.reports.other ?? null]
    .filter((r) => r !== null)
    .join("  ·  ");
  return (
    <article className="page rd-page" lang="en" aria-labelledby={id("rd-title")}>
      <p className="page-court">
        {j.court === "Supreme Court" ? "In the Supreme Court of India" : j.court}
      </p>
      <p className="page-case">{keepIds(reports)}</p>
      <div className="page-parties">
        {j.parties.map((p, i) => (
          <div key={p.name} className="contents">
            {i > 0 && <p className="page-versus">Versus</p>}
            <p className="page-party">
              <span>{p.name}</span>
              <span className="page-role">{p.role === "Respondents" ? "Respondents" : p.role}</span>
            </p>
          </div>
        ))}
      </div>
      <h2 className="page-title rd-title" id={id("rd-title")}>
        Judgment
      </h2>
      <p className="rd-bench">
        <BenchMark bench={bench} stance={j.stance} />
        <span>
          {`${j.bench === 3 ? "Three judges" : j.bench === 2 ? "Two judges" : "Bench"}. Decided ${j.decided}.`}
        </span>
      </p>
      <ol className="rd-paras">
        {j.paras.map((p) => {
          const quoted = p.n === j.quoted.para;
          return (
            <li key={p.n} className="rd-para" data-quoted={quoted ? "" : undefined} value={p.n}>
              <span className="rd-num" aria-hidden="true">
                {p.n}
              </span>
              {quoted ? (
                <div className="rd-quoted" ref={quotedRef}>
                  <p className="rd-quoted-label" lang={w.lang}>
                    {w.r("Quoted in the answer")}
                  </p>
                  <p>
                    {markRuns(keepIds(p.text), j.quoted.words.map(keepIds)).map((r, i) =>
                      r.marked ? <mark key={i}>{r.text}</mark> : <span key={i}>{r.text}</span>,
                    )}
                  </p>
                </div>
              ) : (
                <p>{keepIds(p.text)}</p>
              )}
            </li>
          );
        })}
      </ol>
    </article>
  );
}

/** The judgment reader: the frame, the sheet, and the quoted paragraph pinned to the top once it scrolls away. */
export function JudgmentReader({
  judgment: j,
  open,
  from,
  backRef,
  onBack,
}: {
  judgment: Judgment;
  open: boolean;
  from: DOMRect | null;
  backRef: RefObject<HTMLButtonElement | null>;
  onBack: () => void;
}) {
  const w = useReaderWords();
  const desk = useRef<HTMLDivElement>(null);
  const quoted = useRef<HTMLDivElement>(null);
  const pin = useRef<HTMLButtonElement>(null);

  // it opens at the quoted paragraph, set a little below the desk's top, before the frame paints
  useLayoutEffect(() => {
    const d = desk.current;
    const q = quoted.current;
    if (!open || d === null || q === null) return;
    d.scrollTop = Math.max(
      0,
      q.getBoundingClientRect().top - d.getBoundingClientRect().top + d.scrollTop - 96,
    );
  }, [open, j.id]);

  // the pin shows and hides on the element itself, so a scroll never costs a commit
  useEffect(() => {
    const d = desk.current;
    if (!open || d === null) return;
    const check = () => {
      const q = quoted.current;
      const p = pin.current;
      if (q === null || p === null) return;
      const r = q.getBoundingClientRect();
      const top = d.getBoundingClientRect().top;
      const away = r.bottom < top + 8 || r.top > d.getBoundingClientRect().bottom - 8;
      if (p.hidden === !away) return;
      p.hidden = !away;
    };
    check();
    d.addEventListener("scroll", check, { passive: true });
    return () => d.removeEventListener("scroll", check);
  }, [open, j.id]);

  const back = () => {
    const d = desk.current;
    const q = quoted.current;
    if (d === null || q === null) return;
    d.scrollTop = Math.max(
      0,
      q.getBoundingClientRect().top - d.getBoundingClientRect().top + d.scrollTop - 96,
    );
  };

  const first = j.paras.find((p) => p.n === j.quoted.para)?.text ?? "";
  return (
    <ReaderFrame
      open={open}
      from={from}
      label={w.r("{name}, the judgment", { name: j.name })}
      backRef={backRef}
      deskRef={desk}
      onBack={onBack}
      what={
        <>
          <BenchMark bench={benchOf("court", j.bench)} stance={j.stance} />
          <b lang="en">{j.short}</b>
          <Chip role={j.stance === "against" ? "cite" : "for"} />
        </>
      }
      pinned={
        <button type="button" ref={pin} className="rd-pin" hidden onClick={back}>
          <span className="rd-pin-who">
            <Say>{`${w.r("Quoted in the answer")}, ${w.r("Para {n}", { n: j.quoted.para })}`}</Say>
          </span>
          <span className="rd-pin-text" lang="en">
            {keepIds(j.quoted.words[0] ?? first)}
          </span>
          <span className="sr-only">
            {w.r("Paragraph {n}, quoted in the answer. Scroll back to it", { n: j.quoted.para })}
          </span>
        </button>
      }
    >
      <JudgmentSheet j={j} quotedRef={quoted} />
    </ReaderFrame>
  );
}

/** An entry of the CCI and NCLAT index, in the same frame: the snapshot's date, that it is an index and not a record,
 * and the entry's fields. Its own words are the index's, in English. */
export function IndexReader({
  entry,
  snapshot: date,
  open,
  from,
  backRef,
  onBack,
  onOpenOrder,
}: {
  entry: IndexEntry;
  /** The snapshot's date, in words, as the app gives it. */
  snapshot: string;
  open: boolean;
  from: DOMRect | null;
  backRef: RefObject<HTMLButtonElement | null>;
  onBack: () => void;
  /** Opens the order itself on the CCI's website. */
  onOpenOrder?: () => void;
}) {
  const w = useReaderWords();
  const desk = useRef<HTMLDivElement>(null);
  const snapshot = w.r("Snapshot of {date}", { date });
  return (
    <ReaderFrame
      open={open}
      from={from}
      label={`${w.r("CCI and NCLAT index")}. ${entry.title}`}
      backRef={backRef}
      deskRef={desk}
      onBack={onBack}
      what={
        <>
          <b>
            <Say>{w.r("CCI and NCLAT index")}</Say>
          </b>
          <span className="rd-what-quiet">
            <Say>{snapshot}</Say>
          </span>
        </>
      }
    >
      <article className="page rd-page rd-index">
        <p className="rd-index-kind">
          <Say>{`${w.r("Index entry")}  ·  ${snapshot}`}</Say>
        </p>
        <h2 className="rd-index-title" lang="en">
          {entry.title}
        </h2>
        <p className="rd-index-note">
          <Say>{w.r("An index, not a record. Read the order itself before relying on it.")}</Say>
        </p>
        <dl className="rd-index-rows">
          {entry.rows.map((r) => (
            <div key={r.label} className="rd-index-row">
              <dt>{w.r(r.label)}</dt>
              <dd lang="en">{keepIds(r.value)}</dd>
            </div>
          ))}
        </dl>
        <div className="rd-index-foot">
          <Button kind="tone" size="small" onClick={() => onOpenOrder?.()}>
            {w.r("Open the order on cci.gov.in")}
          </Button>
        </div>
      </article>
    </ReaderFrame>
  );
}
