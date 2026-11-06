// The step group: everything Clauseo did between two of its texts, folded into one quiet line in the thread. Folded,
// the line names the work in lawyers' words with the icons of what was done most, "4 papers read. Searched 2 times.",
// and keeps a failure in view, led by the break; live, it says what is being done now. A press opens it in place to
// its rows, one line each, set into the well as the design's work list is: the icon of what it did, its words (a paper
// by its file name, its folder on hover) and, where it didn't simply finish, its state, led by its clock time in the
// work list's time column once it has one. No animation, and no card.
// The words come in made by the caller, in the language; the rows are the caller's StepRow and StepThought.
import { useId, useState, type ReactElement, type ReactNode } from "react";
import { IconBreak, IconFold, IconNeedsPermission, IconQuestion } from "../icons/icons.tsx";
import { Glued } from "./glued.tsx";
import { Say } from "./say.tsx";
import { ToolKindIcon } from "./tool-kind-icons.tsx";

export type StepGroupProps = {
  /** The group's first entry, for its data attribute. */
  id: string;
  /** The kinds the line draws an icon for, the most done first (at most three are drawn). */
  kinds: readonly string[];
  /** The folded line's words: the summary, or live what is being done now. */
  line: string;
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
   * the time (the design's chip), and its rows open under that line; where it doesn't fit there, the whole line
   * wraps under them, its words wrapping too. None: the line stands on its own. */
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
  // nothing is ever cut: the words wrap, in either language, at any width, and the chevron stays with the last word,
  // so it never stands alone at a line's start
  const chev = (
    <span className="step-inline-icon step-group-chev">
      <IconFold size={12} />
    </span>
  );
  // the last of the words holds the chevron
  const last = trouble !== "" ? "trouble" : more !== "" ? "more" : "line";
  const say = (words: string, here: typeof last) =>
    here === last ? <Glued after={chev}>{words}</Glued> : <Say>{words}</Say>;
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
          <span className="step-group-words">
            {line !== "" && <span className="step-group-now">{say(line, "line")}</span>}
            {more !== "" && (
              <>
                {line !== "" && " "}
                <span className="step-group-more">{say(more, "more")}</span>
              </>
            )}
            {trouble !== "" && (
              <>
                {(line !== "" || more !== "") && " "}
                <span className="step-group-trouble" data-testid="step-group-trouble">
                  {/* the break marks it before it's read, as on its row, centred on the words' own line, and held to
                      its first word */}
                  <Glued
                    before={
                      <span className="step-inline-icon step-trouble-icon">
                        <IconBreak size={14} />
                      </span>
                    }
                    after={last === "trouble" ? chev : undefined}
                  >
                    {trouble}
                  </Glued>
                </span>
              </>
            )}
            {line === "" && more === "" && trouble === "" && chev}
          </span>
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
  /** Its clock time in the thread's words, first, in the work list's time column, and the ISO form for `<time>`; null while it runs. */
  time?: { text: string; iso: string } | null;
  /** What came back (U17): its press and, while open, its block, drawn after the words and the mark; none for a row
   * with nothing to show. */
  result?: ReactNode;
  /** Whether that block is open, as the row's data-result; undefined for a row with no result. */
  resultOpen?: boolean | undefined;
  /** What came back was an error: the break and the mark's primary ink, whatever the status says. */
  failed?: boolean;
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
  time = null,
  result,
  resultOpen,
  failed = false,
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
      data-result={resultOpen === undefined ? undefined : resultOpen ? "open" : "closed"}
      data-failed={failed ? true : undefined}
    >
      {time !== null && (
        <time className="step-row-time" dateTime={time.iso}>
          {time.text}
        </time>
      )}
      <ToolKindIcon
        kind={kind}
        failed={failed || status === "failed"}
        size={14}
        className="step-icon"
      />
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
        {result}
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
