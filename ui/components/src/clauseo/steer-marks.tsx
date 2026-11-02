// What an answer shows of the lawyer's words sent into it while it ran (U7, matters/steer-claude and steer-aside): the
// steered words where they arrived, and the note on a search they moved to the background.
import { type ReactNode } from "react";
import { Button } from "../ui/button.tsx";
import { IconChecking, IconFold } from "../icons/icons.tsx";
import { useMatterWords } from "../lib/words-matters.ts";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { Say } from "./say.tsx";

/** Where an answer shows what happened while it ran: the lawyer's words came in during a search, which carried on in
 * the background and was used. A press opens what the search found, with times. */
export function SteerNote({
  text,
  found,
}: {
  text: string;
  found: readonly { at: string; what: string }[];
}) {
  const w = useMatterWords();
  return (
    <BaseCollapsible.Root className="steer-note">
      <div className="steer-note-row">
        <IconChecking size={16} className="change-icon" />
        <p className="steer-note-text">
          <Say>{text}</Say>
        </p>
        <BaseCollapsible.Trigger
          className="steer-note-more"
          render={<Button kind="quiet" size="small" />}
        >
          <span className="deva-nudge">{w.m("Show what it found")}</span>
          <IconFold size={12} className="work-chev" />
        </BaseCollapsible.Trigger>
      </div>
      <CollapsiblePanel>
        <ul className="work-list steer-note-list">
          {found.map((f) => (
            <li key={f.what} className="work-item">
              <span className="work-at">{f.at}</span>
              <span>
                <Say>{f.what}</Say>
              </span>
            </li>
          ))}
        </ul>
      </CollapsiblePanel>
    </BaseCollapsible.Root>
  );
}

/** The lawyer's words sent into the running answer, standing in it where they arrived: who and when, then the words at
 * the lawyer's weight. */
export function SteerMark({ who, text }: { who: string; text: ReactNode }) {
  return (
    <div className="steer-mark">
      <p className="steer-mark-who">{who}</p>
      <p className="steer-mark-text">{text}</p>
    </div>
  );
}
