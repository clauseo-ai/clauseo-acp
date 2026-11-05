// New matter's folder warning (FR7, FR8, FR13, FR14): a picked folder on a network drive, or on a drive without file
// permissions, under the form's folder line, why behind its info icon. Its buttons (FR9, FR10) are the form's
// (matter-form.tsx). Part of New matter's chunk (folder-check.tsx), with its words (words-folder.ts).
import "./folder-warning.css";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout, IconNotProtected } from "@clauseo/components/icons/icons.tsx";
import { fr, type FolderKey } from "@clauseo/components/lib/words-folder.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";

/** The readings that warn. */
export type Warned = "network" | "no_permissions";

/** Each warning's words: its name (tooltip and accessible name), title, what shows, and what its info icon says. */
const SAYS: Readonly<Record<Warned, { name: FolderKey; head: FolderKey; info: FolderKey }>> = {
  network: {
    name: "This folder is on a network drive",
    head: "On a network drive",
    info: "Clauseo can't keep Claude and Codex inside a folder on a network drive.",
  },
  no_permissions: {
    name: "This drive doesn't keep file permissions",
    head: "No file permissions",
    info: "Clauseo can't keep Claude and Codex inside a folder on this drive. USB drives often don't keep permissions.",
  },
};

/** The warning under the folder line, said as it appears. */
export function FolderWarning({ kind }: { kind: Warned }) {
  const { lang } = useWords();
  const says = SAYS[kind];
  const name = fr(lang, says.name);
  const info = fr(lang, says.info);
  return (
    <div
      className="folder-warning text-small"
      role="alert"
      aria-label={name}
      data-testid="folder-warning"
      data-kind={kind}
    >
      <Tooltip content={name}>
        <span className="folder-warning-icon">
          <IconNotProtected size={16} />
        </span>
      </Tooltip>
      <p className="folder-warning-text">
        <b className="font-chip text-primary">
          <Say>{fr(lang, says.head)}</Say>
        </b>
        <span className="folder-warning-rest">
          <Say>{fr(lang, "Claude asks before every change. Codex can't work here.")}</Say>
        </span>
        <Tooltip content={info}>
          <button type="button" className="room-info" aria-label={info}>
            <IconAbout size={14} />
          </button>
        </Tooltip>
      </p>
    </div>
  );
}
