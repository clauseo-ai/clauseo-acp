// New matter's folder warning (FR7, FR8, FR13, FR14, FR24): a picked folder on a network drive, or on a drive without
// file permissions, under the form's folder line, why behind its info icon; or, on macOS and Linux, a folder whose name
// has a backslash, where Claude can't start (folder-backslash.ts), said whole with no info icon, naming the folder
// above it that has the backslash, if it is not the folder's own name. Its buttons (FR9, FR10) are the form's
// (matter-form.tsx). Part of New matter's chunk (folder-check.tsx), with its words (words-folder.ts).
import "./folder-warning.css";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout, IconNotProtected } from "@clauseo/components/icons/icons.tsx";
import { fr, type FolderKey } from "@clauseo/components/lib/words-folder.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import type { BackslashAt } from "./folder-backslash.ts";
import { backslashWords } from "./folder-backslash-words.ts";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";

/** The readings that warn. */
export type Warned = "network" | "no_permissions" | "backslash";

/** What a drive's warning says after its title. */
const DRIVE = "Claude asks before every change. ChatGPT can't work here." satisfies FolderKey;

/** Each drive's warning's words: its name (tooltip and accessible name), title, what shows after it, and what its info
 * icon says. A backslash's are backslashWords' (folder-backslash-words.ts). */
const SAYS: Readonly<
  Record<
    Exclude<Warned, "backslash">,
    { name: FolderKey; head: FolderKey; rest: FolderKey; info: FolderKey }
  >
> = {
  network: {
    name: "This folder is on a network drive",
    head: "On a network drive",
    rest: DRIVE,
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on a network drive.",
  },
  no_permissions: {
    name: "This drive doesn't keep file permissions",
    head: "No file permissions",
    rest: DRIVE,
    info: "Clauseo can't keep Claude and ChatGPT inside a folder on this drive. USB drives often don't keep permissions.",
  },
};

/** The warning under the folder line, said as it appears. */
export function FolderWarning({ kind, at }: { kind: Warned; at?: BackslashAt | undefined }) {
  const { lang } = useWords();
  const { platform } = useDisplay();
  const says =
    kind === "backslash"
      ? { ...backslashWords(lang, at ?? { self: true }, platform), info: null }
      : {
          name: fr(lang, SAYS[kind].name),
          title: fr(lang, SAYS[kind].head),
          body: fr(lang, SAYS[kind].rest),
          info: fr(lang, SAYS[kind].info),
        };
  const { name, info } = says;
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
          <Say>{says.title}</Say>
        </b>
        <span className="folder-warning-rest">
          <Say>{says.body}</Say>
        </span>
        {info !== null && (
          <Tooltip content={info}>
            <button type="button" className="room-info" aria-label={info}>
              <IconAbout size={14} />
            </button>
          </Tooltip>
        )}
      </p>
    </div>
  );
}
