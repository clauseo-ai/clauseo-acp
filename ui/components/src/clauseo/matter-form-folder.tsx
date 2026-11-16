// The new matter form's Folder row. Its label, then one set-in field like the name's: a folder icon, the chosen path
// with home as "~" (folder-path.ts), cut short in its middle where it's long (the parent gives way, the folder's own
// name stays) with the whole path under the pointer and to a screen reader, and Choose folder… at its end, which opens
// the computer's own folder picker at once. With nothing chosen the field reads "No folder chosen" and Choose folder…
// is the main button. Under the field: where the host has a place for new matters, a quiet "Use a new folder in
// ~/Documents/Clauseo" on one line, its path cut short in its middle as the field's is and whole under the pointer,
// always offered until it's the choice; once chosen, a line says Clauseo makes that folder. The
// paths are the computer's own, in English in both languages.
import { useId } from "react";
import { Button } from "../ui/button.tsx";
import { fieldLabel } from "../ui/recipes-controls.ts";
import { IconFolder, IconNewMatter } from "../icons/icons.tsx";
import { cn } from "../lib/cn.ts";
import { useMatterFormWords } from "../lib/words-matter-form.ts";
import { splitPath } from "./folder-path.ts";
import { Say } from "./say.tsx";

/** A path on one line: its parent cut short first, its last part kept. Isolated, so a name with right-to-left marks
 * can't reorder the line around it. */
export function PathLine({ path, className }: { path: string; className?: string }) {
  const { head, tail } = splitPath(path);
  return (
    <bdi lang="en" dir="ltr" className={cn("folder-path", className)}>
      {head !== "" && <span className="folder-path-head">{head}</span>}
      {tail !== "" && <span className="folder-path-tail">{tail}</span>}
    </bdi>
  );
}

/** Where the path goes in the offer's words, so the words can sit around a PathLine. */
const SLOT = "\u0000";

/** "Use a new folder in {folder}" on one line: the words stay whole and the path is cut short in its middle, as the
 * row's path is. The path keeps its isolates in the text, so the line reads, and is named, as the words have it. */
function OfferWords({ words, path }: { words: string; path: string }) {
  const at = words.indexOf(SLOT);
  const before = at < 0 ? words : words.slice(0, at);
  const after = at < 0 ? "" : words.slice(at + SLOT.length);
  return (
    <span className="matter-form-offer-line deva-nudge">
      {before !== "" && (
        <span className="matter-form-offer-words">
          <Say>{before}</Say>
        </span>
      )}
      <span className="matter-form-offer-path">
        {"\u2068"}
        <PathLine path={path} />
        {"\u2069"}
      </span>
      {after !== "" && (
        <span className="matter-form-offer-words">
          <Say>{after}</Say>
        </span>
      )}
    </span>
  );
}

export function FolderRow({
  shown,
  made,
  offer,
  picking,
  wrong,
  onChoose,
  onOffer,
  testIds,
}: {
  /** The chosen folder as the row shows it, home as "~"; null: none chosen yet. */
  shown: string | null;
  /** Whether the chosen folder is the new one Clauseo makes. */
  made: boolean;
  /** Where "Use a new folder in" makes it, home as "~"; null where the host has no such place, or it's chosen. */
  offer: string | null;
  /** The picker is open: Choose folder… waits for it. */
  picking: boolean;
  /** The line under the row that says what's wrong with the folder, by id, for the row's description. */
  wrong?: string | undefined;
  onChoose: () => void;
  onOffer: () => void;
  testIds?: { choose?: string | undefined; offer?: string | undefined } | undefined;
}) {
  const w = useMatterFormWords();
  const labelId = useId();
  const pathId = useId();
  const madeId = useId();
  return (
    <div
      role="group"
      aria-labelledby={labelId}
      className="matter-form-folder"
      data-chosen={shown === null ? undefined : ""}
    >
      <span id={labelId} className={fieldLabel}>
        <Say>{w("Folder")}</Say>
      </span>
      <div className="matter-form-folder-field" {...(shown === null ? {} : { title: shown })}>
        <IconFolder size={16} className="matter-form-folder-icon" />
        <p id={pathId} className="matter-form-folder-path" data-testid="matter-folder-path">
          {shown === null ? (
            <span className="matter-form-folder-none">
              <Say>{w("No folder chosen")}</Say>
            </span>
          ) : (
            // matter-form-path: the class the e2e flow and the perf autoplay read the chosen folder by
            <PathLine path={shown} className="matter-form-path" />
          )}
        </p>
        <Button
          kind={shown === null ? "primary" : "tone"}
          size="small"
          className="matter-form-choose"
          disabled={picking}
          aria-describedby={[pathId, made ? madeId : null, wrong ?? null]
            .filter((id) => id !== null)
            .join(" ")}
          data-testid={testIds?.choose}
          onClick={onChoose}
        >
          <span className="deva-nudge">{w("Choose folder…")}</span>
        </Button>
      </div>
      {made && (
        <p id={madeId} className="matter-form-folder-note">
          <Say>{w("Clauseo creates this folder. Add papers to it any time.")}</Say>
        </p>
      )}
      {offer !== null && (
        <Button
          kind="quiet"
          size="small"
          className="matter-form-offer"
          title={offer}
          data-testid={testIds?.offer}
          onClick={onOffer}
        >
          <IconNewMatter size={16} />
          <OfferWords words={w("Use a new folder in {folder}", { folder: SLOT })} path={offer} />
        </Button>
      )}
    </div>
  );
}
