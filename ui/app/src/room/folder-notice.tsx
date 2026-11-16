// The room's notice for a matter folder Clauseo can't fence (folder rights): at the top of the work column, once per
// matter until Close. A folder whose permissions the lawyer can't change says Clauseo asks before each edit (FR1),
// with why, and where it needn't ask, behind its info icon (FR2). A network drive (FR12) and a drive without file permissions (FR15) say that Claude asks before every
// change and ChatGPT can't work there, how to get full protection behind the info icon. A folder the host couldn't
// check says Claude asks before every change, in one line. Loaded only for such a matter (work.tsx), with its words
// (words-folder.ts).
import "./folder-notice.css";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import {
  IconAbout,
  IconCross,
  IconNeedsPermission,
  IconNotProtected,
} from "@clauseo/components/icons/icons.tsx";
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
    info: "Clauseo can't add its protection to this folder, so it asks before each edit. In a folder you made yourself on this computer, such as one in Documents, it doesn't need to ask.",
  },
  network: {
    name: "This matter's folder is on a network drive",
    head: "On a network drive.",
    rest: "Claude asks before every change. ChatGPT can't work here.",
    info: "Move the matter to a folder on this computer to use ChatGPT here and let Claude work without asking before every change. Clauseo can't keep them inside a folder on a network drive.",
  },
  no_permissions: {
    name: "This matter's folder is on a drive that doesn't keep file permissions",
    head: "No file permissions.",
    rest: "Claude asks before every change. ChatGPT can't work here.",
    info: "Move the matter to a folder on this computer to use ChatGPT here and let Claude work without asking before every change. Clauseo can't keep them inside this folder, and USB drives often don't keep file permissions.",
  },
  check_failed: {
    name: "Clauseo couldn't check this folder, so Claude asks before every change.",
    head: "Clauseo couldn't check this folder, so Claude asks before every change.",
    rest: null,
    info: null,
  },
};

/** An explanation behind an info icon, named with its words, which also show under the pointer and on focus (as the
 * card's, folder-reason.tsx, kept apart so the two chunks share nothing). Below the icon: above, it would cover the
 * toolbar's matter title. */
function FolderInfo({ words }: { words: string }) {
  return (
    <Tooltip content={words} side="bottom">
      <button type="button" className="room-info" aria-label={words}>
        <IconAbout size={14} />
      </button>
    </Tooltip>
  );
}

export function FolderNotice({ kind, onClose }: { kind: FolderNoted; onClose: () => void }) {
  const { lang } = useWords();
  const says = SAYS[kind];
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
    </div>
  );
}
