// The step group: everything Clauseo did between two of its texts, folded into one quiet line in the thread. Folded,
// the line names the work in lawyers' words with the icons of what was done most, "4 papers read. Searched 2 times.",
// and keeps a failure in view, led by the break; live, it says what is being done now. A press opens it in place to its rows, one line
// each: the icon of what it did, its words (a paper by its file name, its folder on hover) and, where it didn't
// simply finish, its state. No surface and no animation: it is a line between the answer's paragraphs, not a card.
// The words come in made by the caller, in the language; the rows are the caller's StepRow and StepThought.
import { useId, useState, type ReactNode } from "react";
import { IconBreak, IconFold } from "../icons/icons.tsx";
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
  /** Opened when it mounts (the gallery's open state). */
  defaultOpen?: boolean;
  /** The rows, shown while open. */
  children: ReactNode;
};

/** The group: its line, a button that opens and folds it, and its rows under it while open. */
export function StepGroup({
  id,
  kinds,
  line,
  more = "",
  trouble = "",
  live = false,
  defaultOpen = false,
  children,
}: StepGroupProps) {
  const [open, setOpen] = useState(defaultOpen);
  const rows = useId();
  return (
    <div
      className="step-group"
      data-testid="step-group"
      data-entry-id={id}
      data-live={live ? true : undefined}
      data-trouble={trouble === "" ? undefined : true}
    >
      <button
        type="button"
        className="step-group-line"
        aria-expanded={open}
        aria-controls={open ? rows : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="step-group-icons">
          {kinds.slice(0, live ? 1 : 3).map((kind) => (
            <ToolKindIcon key={kind} kind={kind} size={14} className="step-icon" />
          ))}
        </span>
        <span className="step-group-words">
          <span className="step-group-now">
            <Say>{line}</Say>
          </span>
          {more !== "" && (
            <>
              {" "}
              <span className="step-group-more">
                <Say>{more}</Say>
              </span>
            </>
          )}
          {trouble !== "" && (
            <>
              {" "}
              <span className="step-group-trouble" data-testid="step-group-trouble">
                {/* the break marks it before it's read, as on its row */}
                <IconBreak size={14} className="step-trouble-icon" />
                <Say>{trouble}</Say>
              </span>
            </>
          )}
        </span>
        <IconFold size={12} className="step-group-chev" />
      </button>
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
