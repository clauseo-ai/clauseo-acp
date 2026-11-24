// The change note's papers, one row each, when the caller gives each paper its own way to open it (R24): the paper's
// name, and beside it the press the caller names for it ("Open in Word", "Open in Excel", "Open", or Show in Finder for
// a kind no program opens), or nothing; each press is described by its paper's name, for a screen reader. Past SHOWN rows the rest fold under "{n} more", so a turn that changed many
// papers keeps the note short; "Show fewer" folds them again.
import { useId, useState } from "react";
import { Button } from "../ui/button.tsx";
import { IconFolder, IconOpen } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { mt } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

/** A paper's own press on the note: its words, what it does (open the paper, or show its folder), and its test id. */
export type PaperOpen = { label: string; onOpen: () => void; reveal?: boolean; testId?: string };

/** The rows shown before the rest fold. */
const SHOWN = 6;

export function ChangePapers({
  papers,
  opens,
  paperTestId,
}: {
  papers: readonly string[];
  /** Each paper's press, by its place in papers; null for a paper with none. */
  opens: readonly (PaperOpen | null)[];
  paperTestId?: string | undefined;
}) {
  const { lang } = useDisplay();
  const [all, setAll] = useState(false);
  const folds = papers.length > SHOWN;
  // each press names its paper to a screen reader: the name's id, by its row
  const ids = useId();
  const shown = folds && !all ? papers.slice(0, SHOWN) : papers;
  return (
    <>
      <ul className="change-list">
        {shown.map((p, i) => {
          const open = opens[i] ?? null;
          return (
            <li key={`${i}:${p}`} className="change-row">
              <span className="change-name" id={`${ids}-${i}`} data-testid={paperTestId}>
                <bdi>
                  <Say>{p}</Say>
                </bdi>
              </span>
              {open !== null && (
                <Button
                  kind="quiet"
                  size="small"
                  onClick={open.onOpen}
                  data-testid={open.testId}
                  data-paper={p}
                  aria-describedby={`${ids}-${i}`}
                >
                  {open.reveal === true ? <IconFolder size={16} /> : <IconOpen size={16} />}
                  <span className="deva-nudge">{open.label}</span>
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      {folds && (
        <Button
          kind="quiet"
          size="small"
          className="change-more"
          aria-expanded={all}
          onClick={() => setAll(!all)}
          data-testid="change-more"
        >
          <span className="deva-nudge">
            {all ? mt(lang, "Show fewer") : mt(lang, "{n} more", { n: papers.length - SHOWN })}
          </span>
        </Button>
      )}
    </>
  );
}
