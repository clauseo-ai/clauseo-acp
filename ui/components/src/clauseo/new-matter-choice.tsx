// New matter's first question, in its dialog: a new folder or an existing one, two choices on the work's surface, each
// its name and a line on what it means for the lawyer's files. New folder opens the form for a folder Clauseo makes;
// Existing folder opens the computer's own folder picker at once, and the form only once a folder is picked (the
// caller's onChoose). Back from either form comes here again, focus on the choice that led there.
import { useEffect, useRef } from "react";
import { IconFolder, IconNewMatter } from "../icons/icons.tsx";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import type { Branch } from "./matter-form.tsx";
import { PickerLine } from "./picker-line.tsx";
import { Say } from "./say.tsx";

export function NewMatterChoice({
  onChoose,
  last,
  picking = false,
  opening = false,
  testIds,
}: {
  onChoose: (branch: Branch) => void;
  /** Optional: the answer chosen before Back: focus starts on it. */
  last?: Branch | null | undefined;
  /** Optional: the picker Existing folder opened is up: Existing folder waits for it. */
  picking?: boolean | undefined;
  /** Optional: the picker is slow to come: the muted line says it's on its way. */
  opening?: boolean | undefined;
  testIds?: { made?: string; existing?: string } | undefined;
}) {
  const w = useMatterFormWords();
  const made = useRef<HTMLButtonElement>(null);
  const existing = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (last === "new") made.current?.focus();
    if (last === "existing") existing.current?.focus();
  }, [last]);
  return (
    <div className="matter-choice">
      <button
        ref={made}
        type="button"
        className="matter-choice-card"
        data-testid={testIds?.made}
        onClick={() => onChoose("new")}
      >
        <IconNewMatter size={20} className="matter-choice-icon" />
        <span className="matter-choice-name">
          <Say>{w("New folder")}</Say>
        </span>
        <span className="matter-choice-line">
          <Say>
            {w("Clauseo makes an empty folder for this matter. Add papers to it any time.")}
          </Say>
        </span>
      </button>
      <button
        ref={existing}
        type="button"
        className="matter-choice-card"
        // aria-disabled, not disabled: a disabled button would drop focus, and a cancelled picker leaves it here
        aria-disabled={picking ? true : undefined}
        aria-busy={picking ? true : undefined}
        data-testid={testIds?.existing}
        onClick={() => {
          if (!picking) onChoose("existing");
        }}
      >
        <IconFolder size={20} className="matter-choice-icon" />
        <span className="matter-choice-name">
          <Say>{w("Existing folder")}</Say>
        </span>
        <span className="matter-choice-line">
          <Say>
            {w(
              "Use a folder that already holds the papers. It stays where it is, and nothing is moved or copied.",
            )}
          </Say>
        </span>
      </button>
      {opening && <PickerLine />}
    </div>
  );
}
