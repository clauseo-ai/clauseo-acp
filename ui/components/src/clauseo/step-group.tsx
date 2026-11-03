// The step group: everything Clauseo did between two of its texts, folded into one quiet line in the thread. Folded,
// the line names the work in lawyers' words with the icons of what was done most, "4 papers read. Searched 2 times.",
// and keeps a failure in view, led by the break; live, it says what is being done now. A press opens it in place to
// its rows, one line each, set into the well as the design's work list is: the icon of what it did, its words (a paper
// by its file name, its folder on hover) and, where it didn't simply finish, its state. No animation, and no card.
// The words come in made by the caller, in the language; the rows are the caller's StepRow and StepThought.
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
  type RefObject,
} from "react";
import { IconBreak, IconFold, IconNeedsPermission, IconQuestion } from "../icons/icons.tsx";
import { Say } from "./say.tsx";
import { ToolKindIcon } from "./tool-kind-icons.tsx";

export type StepGroupProps = {
  /** The group's first entry, for its data attribute. */
  id: string;
  /** The kinds the line draws an icon for, the most done first (at most three are drawn). */
  kinds: readonly string[];
  /** The folded line's words: the summary, or live what is being done now. */
  line: string;
  /** The summary's sentences, the most important first ("4 papers read.", …, "And 3 more."), when `line` is made of
   * them: on the meta line those that don't fit drop from the end, at a sentence, with "…". */
  parts?: readonly string[] | undefined;
  /** Muted words after the line: live, how many are done so far; "" for none. */
  more?: string;
  /** What failed or was stopped, kept in view on the line; "" for nothing. */
  trouble?: string;
  /** Whether the group is the running one. */
  live?: boolean;
  /** Live, the step in hand waits on the lawyer: the line leads with the permission's shield or the question mark,
   * as the working line does, in place of the kind icons. */
  asks?: "permission" | "question" | undefined;
  /** Opened when it mounts (the gallery's open state). */
  defaultOpen?: boolean;
  /** Wraps the line where it stands: an answer's first group puts it on the answer's meta line, after the name and
   * the time (the design's chip), and its rows open under that line. None: the line stands on its own. */
  head?: ((line: ReactElement) => ReactNode) | undefined;
  /** The rows, shown while open. */
  children: ReactNode;
};

const same = (line: ReactElement) => line;

/** The group: its line, a button that opens and folds it, and its rows under it while open. */
export function StepGroup({
  id,
  kinds,
  line,
  parts,
  more = "",
  trouble = "",
  live = false,
  asks,
  defaultOpen = false,
  head,
  children,
}: StepGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  const rows = useId();
  // on the meta line the line keeps one line, live and folded alike, so the answer never moves as the turn ends: its
  // words cut with an ellipsis where they don't fit, the chevron after them, and the whole summary on hover (its words,
  // all still in the button, are its name)
  const oneLine = head !== undefined;
  const sentences = parts ?? (line === "" ? [] : [line]);
  const { words, keep } = useFit(oneLine, sentences, more, trouble);
  // the muted aside and the trouble, after the line's words
  const aside = (
    <>
      {more !== "" && (
        <>
          {line !== "" && " "}
          <span className="step-group-more">
            <Say>{more}</Say>
          </span>
        </>
      )}
      {trouble !== "" && (
        <>
          {(line !== "" || more !== "") && " "}
          <span className="step-group-trouble" data-testid="step-group-trouble">
            {/* the break marks it before it's read, as on its row, centred on the words' own line */}
            <span className="step-inline-icon step-trouble-icon">
              <IconBreak size={14} />
            </span>
            <Say>{trouble}</Say>
          </span>
        </>
      )}
    </>
  );
  const chev = (
    <span className="step-inline-icon step-group-chev">
      <IconFold size={12} />
    </span>
  );
  return (
    <div
      className="step-group"
      data-testid="step-group"
      data-entry-id={id}
      data-live={live ? true : undefined}
      data-trouble={trouble === "" ? undefined : true}
    >
      {(head ?? same)(
        <button
          type="button"
          className="step-group-line"
          aria-expanded={open}
          aria-controls={open ? rows : undefined}
          title={oneLine ? [line, more, trouble].filter((t) => t !== "").join(" ") : undefined}
          onClick={() => setOpen((o) => !o)}
        >
          {/* none when nothing finished and the trouble leads: the break alone stands first */}
          {asks !== undefined && (
            <span className="step-group-icons" data-asks={asks}>
              {asks === "permission" ? (
                <IconNeedsPermission size={14} className="step-icon" />
              ) : (
                <IconQuestion size={14} className="step-icon" />
              )}
            </span>
          )}
          {kinds.length > 0 && (
            <span className="step-group-icons">
              {kinds.slice(0, live ? 1 : 3).map((kind) => (
                <ToolKindIcon key={kind} kind={kind} size={14} className="step-icon" />
              ))}
            </span>
          )}
          <span className="step-group-words" ref={words}>
            {line !== "" && (
              <span className="step-group-now">
                {oneLine ? <Sentences sentences={sentences} keep={keep} /> : <Say>{line}</Say>}
              </span>
            )}
            {!oneLine && aside}
            {/* the chevron follows the last word, so it stays beside the words when they wrap */}
            {!oneLine && chev}
          </span>
          {/* on one line the aside and the trouble stand in their own slot after the summary, never clipped: a cut
              falls on the summary alone; the chevron after them */}
          {oneLine && (more !== "" || trouble !== "") && (
            <span className="step-group-kept">{aside}</span>
          )}
          {oneLine && chev}
        </button>,
      )}
      {open && (
        <ol id={rows} className="step-group-rows" data-testid="step-group-rows">
          {children}
        </ol>
      )}
    </div>
  );
}

/** The summary's sentences on the meta line: the first `keep` seen, then "…" where any are dropped; the dropped
 * ones stay in the button's name, unseen, so its name is the whole summary. The "…" is drawn by CSS, so it is in no
 * text, and stands unseen out of the flow while nothing is dropped. */
function Sentences({ sentences, keep }: { sentences: readonly string[]; keep: number }) {
  return (
    <>
      {sentences.map((s, i) => (
        <span
          key={i}
          className={i < keep ? "step-part" : "step-part step-part-dropped"}
          data-part={i}
        >
          {i > 0 && " "}
          <Say>{s}</Say>
        </span>
      ))}
      <span
        className="step-part-cut"
        aria-hidden="true"
        data-cut={keep < sentences.length ? true : undefined}
      />
    </>
  );
}

/** How many of the summary's sentences fit on the meta line, before the aside and the trouble, which always stay in
 * their own slot: all,
 * or fewer, dropped from the end with "…" after the last kept; at least one (a narrower line cuts it mid-word, the
 * CSS ellipsis, as a last resort). Measured when the words change and when the meta line's width does (a
 * ResizeObserver), never as the answer's text streams; a line that fits commits nothing more, and one that doesn't one
 * commit more. */
function useFit(
  on: boolean,
  sentences: readonly string[],
  more: string,
  trouble: string,
): { words: RefObject<HTMLSpanElement | null>; keep: number } {
  const words = useRef<HTMLSpanElement | null>(null);
  const [width, setWidth] = useState(0);
  const key = `${sentences.join("\n")}\n${more}\n${trouble}\n${width}`;
  // a cut, for the words and the width it was measured at; any other words or width show every sentence, so a line
  // that fits needs no state at all
  const [fit, setFit] = useState<{ key: string; keep: number } | null>(null);
  const measured = useRef<string | null>(null);
  const n = sentences.length;
  const keep = fit !== null && fit.key === key ? fit.keep : null;
  useLayoutEffect(() => {
    const el = words.current;
    if (!on || el === null || measured.current === key) return;
    // one sentence has nothing to drop; a hidden page measures when the words or the width next change
    if (n < 2) {
      measured.current = key;
      return;
    }
    if (document.visibilityState === "hidden") return;
    measured.current = key;
    if (el.scrollWidth <= el.clientWidth + 0.5) return;
    // the "…" stands unseen out of the flow until it's wanted, so its width is there to read
    const cut = el.querySelector<HTMLElement>(".step-part-cut")?.getBoundingClientRect().width ?? 0;
    const widths = [...el.querySelectorAll<HTMLElement>(".step-part")].map(
      (p) => p.getBoundingClientRect().width,
    );
    // the words' box holds the summary alone; what isn't a sentence (spaces) and the "…" take the rest
    let used = el.scrollWidth - widths.reduce((x, y) => x + y, 0) + cut;
    let k = 0;
    while (k < n - 1 && used + widths[k]! <= el.clientWidth) used += widths[k++]!;
    setFit({ key, keep: Math.max(1, k) });
  }, [on, key, n]);
  // the meta line's width: a change measures again
  useEffect(() => {
    const meta = words.current?.closest(".meta");
    if (!on || !meta || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([e]) => {
      const w = Math.round(e?.contentRect.width ?? 0);
      setWidth((was) => (was === w ? was : w));
    });
    ro.observe(meta);
    return () => ro.disconnect();
  }, [on]);
  return { words, keep: keep === null ? n : Math.min(keep, n) };
}

export type StepRowProps = {
  /** The entry's id, the tool call's id and its status, as data attributes. */
  id: string;
  toolCall: string;
  status: string;
  kind: string;
  /** What it did, or is doing, in a sentence. */
  text: string;
  /** Where the paper is (its folder), shown on hover; null for none. */
  hover: string | null;
  /** Its state where it didn't simply finish: Failed, Stopped, Waiting, In the background; null for none. */
  mark: string | null;
  /** It carried on in the background as a steer joined its turn. */
  background?: boolean;
};

/** One row of an open group. */
export function StepRow({
  id,
  toolCall,
  status,
  kind,
  text,
  hover,
  mark,
  background,
}: StepRowProps) {
  return (
    <li
      className="step-row"
      data-testid="tool-call"
      data-entry-id={id}
      data-tool-call-id={toolCall}
      data-status={status}
      data-kind={kind}
      data-background={background ? true : undefined}
    >
      <ToolKindIcon kind={kind} failed={status === "failed"} size={14} className="step-icon" />
      <span className="step-row-text" title={hover ?? undefined}>
        <bdi>
          <Say>{text}</Say>
        </bdi>
        {mark !== null && (
          <>
            {" · "}
            <span className="step-row-mark">
              <Say>{mark}</Say>
            </span>
          </>
        )}
      </span>
    </li>
  );
}

/** A thought in an open group: the thought's own row, beside the think icon. */
export function StepThought({ children }: { children: ReactNode }) {
  return (
    <li className="step-row step-row-thought">
      <ToolKindIcon kind="think" size={14} className="step-icon" />
      <div className="step-row-text">{children}</div>
    </li>
  );
}
