// What an answer shows of the lawyer's words sent into it while it ran (U7, matters/steer-claude and steer-aside): the
// steered words where they arrived, and the note on a search they moved to the background. The note
// opens its list with a plain disclosure, not the base collapsible: the thread's chunk carries it, and a collapsible
// there would become a chunk shared with the working line's details, loaded after the thread's (vite-config.test.ts).
import { type ReactNode, useId, useState } from "react";
import { Button } from "../ui/button.tsx";
import { IconChecking, IconFold } from "../icons/icons.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** Where an answer shows what happened while it ran: the lawyer's words came in during a search, which carried on in
 * the background and was used. A press opens what the search found, with times. */
export function SteerNote({
  text,
  found,
  testId,
}: {
  text: string;
  found: readonly { at: string; what: string }[];
  /** The note's test id, for the app's tests (additive). */
  testId?: string;
}) {
  const w = useMatterWords();
  const [open, setOpen] = useState(false);
  const list = useId();
  return (
    <div className="steer-note" data-testid={testId}>
      <div className="steer-note-row">
        <IconChecking size={16} className="change-icon" />
        <p className="steer-note-text">
          <Say>{text}</Say>
        </p>
        <Button
          kind="quiet"
          size="small"
          className="steer-note-more"
          aria-expanded={open}
          aria-controls={list}
          data-panel-open={open ? "" : undefined}
          onClick={() => setOpen((was) => !was)}
        >
          <span className="deva-nudge">{w.m("Show what it found")}</span>
          <IconFold size={12} className="work-chev" />
        </Button>
      </div>
      <ul id={list} hidden={!open} className="work-list steer-note-list">
        {found.map((f, i) => (
          <li key={i} className="work-item">
            <span className="work-at">{f.at}</span>
            <span>
              <Say>{f.what}</Say>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** The lawyer's words sent into the running answer, standing in it where they arrived: who and when, then the words at
 * the lawyer's weight. */
export function SteerMark({
  who,
  text,
  testId,
  entryId,
}: {
  who: string;
  text: ReactNode;
  /** The mark's test id and the entry it shows, for the app's tests (additive). */
  testId?: string;
  entryId?: string;
}) {
  return (
    <div className="steer-mark" data-testid={testId} data-entry-id={entryId}>
      <p className="steer-mark-who">{who}</p>
      <p className="steer-mark-text">{text}</p>
    </div>
  );
}
