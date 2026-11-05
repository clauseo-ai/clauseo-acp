// The room's notice for a matter folder Clauseo can't fence (folder rights): at the top of the work column, once per
// matter until Close. A folder whose permissions the lawyer can't change says Clauseo asks before each edit (FR1),
// why behind its info icon (FR2), and What to tell IT (FR3), which opens the text to send IT, selected, with Copy
// (FR5). A network drive (FR12) and a drive without file permissions (FR15) say that Claude asks before every
// change and Codex can't work there, how to get full protection behind the info icon. A folder the host couldn't
// check says Claude asks before every change, in one line. Loaded only for such a matter (work.tsx), with its words
// (words-folder.ts).
import "./folder-notice.css";
import { useId, useLayoutEffect, useRef, useState } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import {
  IconAbout,
  IconCross,
  IconNeedsPermission,
  IconNotProtected,
} from "@clauseo/components/icons/icons.tsx";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import { fr, type FolderKey } from "@clauseo/components/lib/words-folder.ts";
import { ui } from "@clauseo/components/lib/words-ui.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { Tooltip } from "@clauseo/components/ui/tooltip.tsx";
import { unprotectedFolder, type FolderNoted } from "./folder-support.ts";

/** Each reading's words: its name (tooltip and accessible name), what shows, and what its info icon says. */
const SAYS: Readonly<
  Record<
    FolderNoted,
    { name: FolderKey; head: FolderKey; rest: FolderKey | null; info: FolderKey | null }
  >
> = {
  cant_change_permissions: {
    name: "Clauseo asks before each edit in this folder",
    head: "Clauseo asks before each edit",
    rest: null,
    info: "You can change files here, but not this folder's permissions. Clauseo needs them to keep Claude and Codex inside the matter. Some edits won't go through.",
  },
  network: {
    name: "This matter's folder is on a network drive",
    head: "On a network drive.",
    rest: "Claude asks before every change. Codex can't work here.",
    info: "Clauseo can't keep Claude and Codex inside this folder. For full protection, move the matter to a folder on this computer.",
  },
  no_permissions: {
    name: "This matter's folder is on a drive that doesn't keep file permissions",
    head: "No file permissions.",
    rest: "Claude asks before every change. Codex can't work here.",
    info: "Clauseo can't keep Claude and Codex inside this folder. USB drives often don't keep permissions. For full protection, move the matter to a folder on this computer.",
  },
  check_failed: {
    name: "Clauseo couldn't check this folder, so Claude asks before every change.",
    head: "Clauseo couldn't check this folder, so Claude asks before every change.",
    rest: null,
    info: null,
  },
};

const IT_TEXT: FolderKey =
  'Please give my account "Change permissions" on this folder: {path}. Clauseo, the app I use for this matter, adds one entry to the folder\'s permissions so Claude and Codex, which it runs, can write only inside it.';

/** An explanation behind an info icon, named with its words, which also show under the pointer and on focus (as the
 * card's, folder-reason.tsx, kept apart so the two chunks share nothing). */
function FolderInfo({ words }: { words: string }) {
  return (
    <Tooltip content={words}>
      <button type="button" className="room-info" aria-label={words}>
        <IconAbout size={14} />
      </button>
    </Tooltip>
  );
}

export function FolderNotice({
  kind,
  folder,
  onClose,
}: {
  kind: FolderNoted;
  /** The matter's folder as the host shows it (folder_display), for the IT text. */
  folder: string;
  onClose: () => void;
}) {
  const { lang } = useWords();
  const says = SAYS[kind];
  const [it, setIt] = useState(false);
  const panel = useId();
  const name = fr(lang, says.name);
  const Icon = unprotectedFolder(kind) ? IconNotProtected : IconNeedsPermission;
  return (
    <div
      className="room-folder"
      role="note"
      aria-label={name}
      data-testid="folder-notice"
      data-kind={kind}
    >
      <div className="room-folder-row">
        <Tooltip content={name}>
          <span className="room-folder-icon">
            <Icon size={16} />
          </span>
        </Tooltip>
        <p className="room-folder-text text-small">
          <b className="font-chip text-primary">
            <Say>{fr(lang, says.head)}</Say>
          </b>
          {says.rest !== null && (
            <span className="room-folder-rest">
              <Say>{fr(lang, says.rest)}</Say>
            </span>
          )}
          {says.info !== null && <FolderInfo words={fr(lang, says.info)} />}
        </p>
        {kind === "cant_change_permissions" && (
          <Button
            kind="tone"
            size="small"
            aria-expanded={it}
            aria-controls={it ? panel : undefined}
            data-testid="folder-it"
            onClick={() => setIt((open) => !open)}
          >
            {fr(lang, "What to tell IT")}
          </Button>
        )}
        <Tooltip content={ui(lang, "Close")}>
          <Button
            kind="quiet"
            size="small"
            shape="icon"
            aria-label={ui(lang, "Close")}
            data-testid="folder-close"
            onClick={onClose}
          >
            <IconCross size={16} />
          </Button>
        </Tooltip>
      </div>
      {it && <ItText id={panel} folder={folder} />}
    </div>
  );
}

/** What to tell IT: the request with the matter's folder, selected as it opens, and Copy. */
function ItText({ id, folder }: { id: string; folder: string }) {
  const { lang } = useWords();
  const text = useRef<HTMLParagraphElement>(null);
  // selected as it opens, so Cmd or Ctrl with C copies it too
  useLayoutEffect(() => {
    const node = text.current;
    const selection = globalThis.getSelection?.();
    if (node === null || selection === null || selection === undefined) return;
    selection.selectAllChildren(node);
  }, []);
  const copy = () => {
    void navigator.clipboard?.writeText(fr(lang, IT_TEXT, { path: folder })).catch(() => {});
  };
  return (
    <div className="room-folder-it" id={id} data-testid="folder-it-text">
      <p ref={text} className="text-small text-primary select-text">
        {fr(lang, IT_TEXT, { path: isolate(folder) })}
      </p>
      <Button kind="tone" size="small" onClick={copy}>
        {fr(lang, "Copy")}
      </Button>
    </div>
  );
}
