// The parts of the usage limit and of what breaks. A note in the thread that says what happened,
// what's finished and what's left, and the ways on; the mark where an answer stopped mid-sentence; the draft's
// paragraphs as they went into Word; a quiet note under the field; a day's line in the thread; a folded exchange.
// Parts hold no strings: every word comes in through props, already in the language. Colour keeps its meaning:
// court yellow only where a choice waits for the lawyer's hand, everything else in concrete, and never red.
import type { ReactNode } from "react";
import { Collapsible, CollapsiblePanel, CollapsibleTrigger } from "../ui/collapsible.tsx";
import { IconChecking, IconGoodLaw, IconPaperBookPage } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { Who } from "./answer.tsx";
import { keepIds } from "./matter.ts";
import { Say } from "./say.tsx";

/** "Today", or "Monday 26 October": a day's line in the thread, where the exchanges below it happened. */
export function DayLine({ children }: { children: string }) {
  return (
    <p className="lim-day">
      <span>
        <Say>{children}</Say>
      </span>
    </p>
  );
}

/** An earlier exchange folded to one line that keeps its result in view: the time, what it made at the lawyer's
 * weight, and where it stands. Open, it shows the question and Clauseo's reply. `accepted` leads the state with a
 * check; otherwise the state is plain words, such as "14 changes waiting in Word". */
export function FoldedExchange({
  at,
  title,
  state,
  accepted = false,
  question,
  reply,
  you,
  clauseo,
  label,
}: {
  at: string;
  title: string;
  state: string;
  accepted?: boolean;
  question: string;
  reply: string;
  you: string;
  clauseo: string;
  label: string;
}) {
  return (
    <section className="step" aria-label={label}>
      <Collapsible>
        <CollapsibleTrigger kind="fold" className="fold-line" title={question}>
          <span className="fold-time">{at}</span>
          <span className="fold-title">
            <Say>{title}</Say>
          </span>
          <span className={cn("fold-state", !accepted && "lim-fold-plain")}>
            {accepted && <IconGoodLaw size={13} />}
            <span>
              <Say>{state}</Say>
            </span>
          </span>
        </CollapsibleTrigger>
        <CollapsiblePanel>
          <div className="fold-body">
            <Who name={you} time={at} />
            <p className="asked">
              <Say>{question}</Say>
            </p>
            <div className="reply">
              <Who name={clauseo} time={at} />
              <p className="reply-text">
                <Say>{reply}</Say>
              </p>
            </div>
          </div>
        </CollapsiblePanel>
      </Collapsible>
    </section>
  );
}

/** Where an answer stopped: a quiet mark straight after its last word, "Stopped here at 4:52 pm". Nothing blinks:
 * it is a label, not a cursor, and it carries no clock, which marks only what still waits. */
export function StopMark({ children }: { children: string }) {
  return (
    <span className="lim-stop">
      <span className="deva-nudge">
        <Say>{children}</Say>
      </span>
    </span>
  );
}

/** The draft's paragraphs as they went into the Word file, on a sheet set into the answer: the file's name above,
 * each paragraph at Clauseo's weight. The last one may stop mid-sentence, and carry the mark where it stopped. */
export function DraftExcerpt({
  file,
  paras,
  stop,
}: {
  file: string;
  paras: readonly string[];
  stop?: string | undefined;
}) {
  return (
    <figure className="lim-draft">
      <figcaption className="lim-draft-head">
        <IconPaperBookPage size={14} className="lim-draft-icon" />
        <span>
          <Say>{file}</Say>
        </span>
      </figcaption>
      <div className="lim-draft-sheet">
        {paras.map((p, i) => (
          <p key={i} className="lim-draft-para">
            <Say>{keepIds(p)}</Say>
            {stop !== undefined && i === paras.length - 1 && (
              <>
                {" "}
                <StopMark>{stop}</StopMark>
              </>
            )}
          </p>
        ))}
      </div>
    </figure>
  );
}

/** A Word file with tracked changes waiting for the lawyer, at the end of an answer: the yellow icon tile, because
 * the changes wait for the lawyer's hand, the file's name, what's in it, and Open in Word as the tonal button. The
 * thread's one form of Open in Word. `className` sets it inside another part, such as a note's Finished row. */
export function WordCard({
  title,
  sub,
  state,
  action,
  className,
}: {
  title: string;
  sub: string;
  state: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("lim-word", className)}>
      <span className="card-icon lim-word-icon">
        <IconPaperBookPage size={18} />
      </span>
      <div className="min-w-0">
        <p className="lim-word-title" lang="en">
          {title}
        </p>
        <p className="lim-word-sub">
          <span>
            <Say>{sub}</Say>
          </span>
          <i className="room-dot" aria-hidden="true" />
          <span className="lim-word-state">
            <Say>{state}</Say>
          </span>
        </p>
      </div>
      {action}
    </div>
  );
}

/** One row of a note: a short label at the left, "Finished" or "Left", and what it says at the right. */
export type NoticeRow = { label: string; text: ReactNode };

/** A note in the thread: what happened, said plainly, and the ways on. On the well tone with 14 px corners, like the
 * Ready card. Its icon tile takes court yellow only while a choice waits for the lawyer (`waiting`); otherwise it
 * stands in concrete. Rows say what's finished and what's left; the actions sit below them, with a line on what the
 * first one does; `other` is another way on, such as the other plan, set off below a hairline. */
export function NoticeCard({
  icon,
  waiting = false,
  title,
  sub,
  rows = [],
  actions,
  note,
  other,
  label,
  className,
  noteFirst = false,
}: {
  icon: ReactNode;
  waiting?: boolean;
  title: string;
  sub?: string | null;
  rows?: readonly NoticeRow[];
  actions?: ReactNode;
  note?: string | null;
  other?: { text: string; action: ReactNode } | null;
  label: string;
  className?: string;
  /** The line before the actions, as a confirmation reads, rather than after them, as an explanation does. */
  noteFirst?: boolean;
}) {
  const line =
    note != null && note !== "" ? (
      <p className="lim-note-line">
        <Say>{note}</Say>
      </p>
    ) : null;
  return (
    <section
      className={cn("lim-note", className)}
      aria-label={label}
      data-waiting={waiting ? "" : undefined}
    >
      <header className="lim-note-head">
        <span className="card-icon lim-note-icon">{icon}</span>
        <div className="min-w-0">
          <p className="lim-note-title">
            <Say>{title}</Say>
          </p>
          {sub != null && sub !== "" && (
            <p className="lim-note-sub">
              <Say>{sub}</Say>
            </p>
          )}
        </div>
      </header>
      {rows.length > 0 && (
        <dl className="lim-note-rows">
          {rows.map((r) => (
            <div key={r.label} className="lim-note-row">
              <dt className="lim-note-label">
                <Say>{r.label}</Say>
              </dt>
              <dd className="lim-note-text">
                {typeof r.text === "string" ? <Say>{r.text}</Say> : r.text}
              </dd>
            </div>
          ))}
        </dl>
      )}
      {(actions != null || (note != null && note !== "")) && (
        <div className="lim-note-go">
          {noteFirst && line}
          {actions != null && <div className="lim-note-acts">{actions}</div>}
          {!noteFirst && line}
        </div>
      )}
      {other != null && (
        <div className="lim-note-other">
          <p className="lim-note-other-text">
            <Say>{other.text}</Say>
          </p>
          {other.action}
        </div>
      )}
    </section>
  );
}

/** A way forward, side by side with another: its name, the account it uses if one is signed in, what it means, and
 * the one action that takes it. The account stands on its own line, so a long address never strands the words
 * before it. */
export function WayTile({
  title,
  account,
  body,
  action,
}: {
  title: string;
  account?: string | null;
  body: string;
  action: ReactNode;
}) {
  return (
    <div className="lim-way">
      <p className="lim-way-title">
        <Say>{title}</Say>
      </p>
      {account != null && account !== "" && (
        <p className="lim-way-account" lang="en">
          {account}
        </p>
      )}
      <p className="lim-way-body">
        <Say>{body}</Say>
      </p>
      <div className="lim-way-go">{action}</div>
    </div>
  );
}

/** A quiet note under the composer's field, above the room's line: plain words, never a gauge. `lead` stands in
 * primary, the rest in the muted tone. `wait` leads it with the clock, where the note says what waits and until
 * when; advice carries no icon, and its words start on the room's line's words. */
export function FieldNotice({
  lead,
  rest,
  action,
  wait = false,
}: {
  lead: string;
  rest?: string | null;
  action?: ReactNode;
  wait?: boolean;
}) {
  return (
    <p className="lim-field-note" role="status" data-wait={wait ? "" : undefined}>
      {wait && <IconChecking size={14} className="lim-field-icon" />}
      <span className="min-w-0">
        <span className="lim-field-lead">
          <Say>{lead}</Say>
        </span>
        {rest != null && rest !== "" && (
          <span className="lim-field-rest">
            <Say>{rest}</Say>
          </span>
        )}
        {action != null && <> {action}</>}
      </span>
    </p>
  );
}

/** A quiet band across the top of the work: what's going on in the whole room, such as being offline, and what
 * still works and what waits. */
export function RoomBand({ title, rows }: { title: string; rows: readonly NoticeRow[] }) {
  return (
    <section className="lim-band" aria-label={title} role="status">
      <p className="lim-band-title">
        <IconChecking size={16} className="lim-band-icon" />
        <span className="deva-nudge">
          <Say>{title}</Say>
        </span>
      </p>
      <dl className="lim-band-rows">
        {rows.map((r) => (
          <div key={r.label} className="lim-band-row">
            <dt className="lim-band-label">
              <Say>{r.label}</Say>
            </dt>
            <dd>{typeof r.text === "string" ? <Say>{r.text}</Say> : r.text}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}
