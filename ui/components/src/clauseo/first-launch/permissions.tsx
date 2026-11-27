// The first screen's permissions step (issue 5): what macOS will ask Clauseo for, asked here first, each with its
// reason, so no system notice stops the lawyer in the middle of a matter. On the Mac at most two rows, in order: the
// Documents folder (where the matters folder sits inside it), then the keychain (where a read of Clauseo's own items
// would prompt). Each row not answered yet has its own Allow; macOS's own dialog comes only after that press, and
// the row waits for the lawyer's answer there. An allowed row shows its tick and is never asked again; a refused one
// says where to change it, in the system's words the desktop gives, and a refused Documents row can also send new
// matters to a folder outside Documents where the host can set one (the main window). On Windows nothing is asked: where Controlled
// folder access blocks the matters folder (or can't be read), the row says so, and its action is a matters folder
// outside Documents; the desktop's settings_path and note are detail under it. The app decides which rows stand
// (room/first-run/permissions.tsx); this draws them.
import { useId } from "react";
import { useFirstLaunchWords, type FirstLaunchKey } from "../../lib/words-first-launch.ts";
import { Button } from "../../ui/button.tsx";
import { Say } from "../say.tsx";
import { FolderChoiceButton, type FolderChoice } from "./folder-choice.tsx";
import { WINDOWS } from "./windows-words.ts";
import { Done } from "./parts.tsx";

/** One row as the step draws it. "ask": not asked yet, with its own Allow. */
export type PermissionStepRow = {
  id: "files_documents" | "keychain" | "controlled_folder_access";
  /** "once": an ask macOS let through once only (plain Allow), so the row would still ask. */
  shows: "ask" | "waiting" | "granted" | "denied" | "failed" | "once" | "blocking" | "unknown";
  /** The folder the row is about, as it reads (Documents/Clauseo). */
  folder?: string | undefined;
  /** Where to change it, in the system's words, as the desktop gives it. */
  path?: string | undefined;
  /** The desktop's own line on it, shown as given. */
  note?: string | undefined;
};

export type PermissionStepActs = {
  /** Asks macOS for the row: one host call per press. */
  onAsk: (id: PermissionStepRow["id"]) => void;
  /** Opens System Settings at Files and Folders; without it, no button. */
  onSettings?: (() => void) | undefined;
  /** Reads the rows again, after the lawyer changed a setting; without it, no button. */
  onCheck?: (() => void) | undefined;
  /** Opens a folder outside Documents as the first matter (Windows), until the host can set a matters folder. */
  onOtherFolder?: (() => void) | undefined;
  /** Where the host can set a matters folder: the picker, then the command. Windows' row offers it ahead of
   * onOtherFolder, and the Mac's refused Documents row beside its ways on. */
  onChooseFolder?: (() => Promise<FolderChoice | null>) | undefined;
  /** Puts the band away for a keychain the lawyer refused (the band's only, until the next start); without it, no
   * button. */
  onNotNow?: (() => void) | undefined;
};

/** The step's heading: the first screen's, or the band's over the room for a lawyer who has matters. */
export type StepHeading = "Before your first matter" | "Before you go on";

/** The step, at the head of the first screen's question, in the band over the room, or in Settings' group. */
export function PermissionsStep({
  rows,
  platform,
  heading = "Before your first matter",
  ...acts
}: {
  rows: readonly PermissionStepRow[];
  platform: "mac" | "windows";
  /** null where the place around it names it (Settings' Permissions group): no heading of its own. */
  heading?: StepHeading | null;
} & PermissionStepActs) {
  const w = useFirstLaunchWords();
  const id = useId();
  return (
    <section
      className="fl-perms"
      aria-labelledby={heading === null ? undefined : id}
      data-testid="permissions-step"
      data-platform={platform}
    >
      {heading !== null && (
        <h2 id={id} className="fl-perms-head">
          <Say>{w(heading)}</Say>
        </h2>
      )}
      {platform === "mac" && (
        <p className="fl-perms-lead">
          <Say>{w("Answer here first, so macOS doesn't stop you in the middle of a matter.")}</Say>
        </p>
      )}
      <ul className="fl-perms-rows">
        {rows.map((row) => (
          <li
            key={row.id}
            className="fl-perm"
            data-testid="permission-row"
            data-id={row.id}
            data-shows={row.shows}
          >
            <Row row={row} {...acts} />
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The band's quiet line for a Documents row the desktop lists not asked again after an update, on a Mac that answered
 * it before: one line, its own Allow. `waiting` while the ask is under way; `failed` when it couldn't be asked. */
export function DocumentsAgain({
  onAsk,
  waiting,
  failed,
}: {
  onAsk: () => void;
  waiting: boolean;
  failed: boolean;
}) {
  const w = useFirstLaunchWords();
  return (
    <section className="fl-perms-again" data-testid="documents-again">
      <p className="fl-perms-again-words">
        <Say>{w("Clauseo was updated. Allow it to open your Documents folder again.")}</Say>
        {failed && (
          <>
            {" "}
            <Say>{w("Clauseo couldn't ask macOS. Try again.")}</Say>
          </>
        )}
      </p>
      <Button
        kind="tone"
        size="small"
        disabled={waiting}
        onClick={onAsk}
        data-testid="documents-again-allow"
      >
        {w("Allow")}
      </Button>
    </section>
  );
}

/** A row's name and reason. */
function about(row: PermissionStepRow): { name: FirstLaunchKey; why: FirstLaunchKey } {
  switch (row.id) {
    case "files_documents":
      return {
        name: "Your Documents folder",
        why: "Your matters' papers live here. Clauseo, Claude and ChatGPT read them and save their work beside them.",
      };
    case "keychain":
      return {
        name: "Your keychain",
        why: "Clauseo keeps your sign-ins and connector passwords in your Mac's keychain. macOS asks once for each and wants your Mac's login password. Choose Always Allow. Plain Allow means macOS asks again next time.",
      };
    case "controlled_folder_access":
      return {
        name: row.shows === "unknown" ? WINDOWS.unknown : WINDOWS.blocking,
        why: WINDOWS.why,
      };
  }
}

/** The desktop's own line, shown as given. */
const Note = ({
  note,
  testId = "permission-note",
}: {
  note: string | undefined;
  testId?: string;
}) =>
  note === undefined ? null : (
    <p className="fl-perm-note" data-testid={testId}>
      {note}
    </p>
  );

function Row({ row, ...acts }: { row: PermissionStepRow } & PermissionStepActs) {
  const w = useFirstLaunchWords();
  const { name, why } = about(row);
  const { shows } = row;
  return (
    <>
      <div className="fl-perm-top">
        <div className="fl-perm-name">
          <p className="fl-perm-title">
            <Say>{w(name)}</Say>
          </p>
          {/* the matters folder Windows blocks; the Mac's row is about Documents, which its name says */}
          {row.folder !== undefined && row.id === "controlled_folder_access" && (
            <p className="fl-perm-folder" data-testid="permission-folder">
              {row.folder}
            </p>
          )}
        </div>
        {/* a refused keychain row keeps its Allow: it can be allowed here at any time */}
        {(shows === "ask" ||
          shows === "failed" ||
          shows === "once" ||
          (shows === "denied" && row.id === "keychain")) && (
          <Button
            kind="tone"
            className="fl-perm-act"
            onClick={() => acts.onAsk(row.id)}
            data-testid="permission-allow"
          >
            {w("Allow")}
          </Button>
        )}
        {shows === "granted" && (
          <Done className="fl-perm-done" testId="permission-granted">
            <Say>{w("Allowed")}</Say>
          </Done>
        )}
      </div>
      <p className="fl-perm-why">
        <Say>{w(why)}</Say>
      </p>
      {shows === "waiting" && (
        <p className="fl-perm-wait" role="status" data-testid="permission-waiting">
          <Say>
            {row.id === "keychain"
              ? w("macOS is asking. Type your Mac's login password, then choose Always Allow.")
              : w("macOS is asking. Choose Allow in its window.")}
          </Say>
        </p>
      )}
      {shows === "failed" && (
        <p className="fl-perm-said" role="status" data-testid="permission-failed">
          <Say>{w("Clauseo couldn't ask macOS. Try again.")}</Say>
        </p>
      )}
      {shows === "once" && (
        <p className="fl-perm-said" role="status" data-testid="permission-once">
          <Say>{w("macOS allowed it once only. Press Allow again and choose Always Allow.")}</Say>
        </p>
      )}
      {shows === "denied" && <Refused row={row} {...acts} />}
      {(shows === "blocking" || shows === "unknown") && (
        <>
          {/* the desktop's detail: where the setting lives, introduced, and its note unless it repeats the reason */}
          {row.path !== undefined && (
            <p className="fl-perm-note" data-testid="permission-path">
              <Say>{w("In Windows Security: {path}", { path: underSecurity(row.path) })}</Say>
            </p>
          )}
          <Note note={row.note === WINDOWS.why ? undefined : row.note} />
          {/* the action after the words: it is the advice they end on */}
          <div className="fl-perm-ways fl-perm-ways-act">
            <WindowsAct {...acts} />
            {acts.onCheck !== undefined && <CheckAgain onCheck={acts.onCheck} />}
          </div>
        </>
      )}
    </>
  );
}

/** Windows' action: the matters-folder choice where the host can set one, else the first matter's picker. */
function WindowsAct({ onChooseFolder, onOtherFolder }: PermissionStepActs) {
  const w = useFirstLaunchWords();
  if (onChooseFolder !== undefined)
    return <FolderChoiceButton kind="tone" onChoose={onChooseFolder} />;
  if (onOtherFolder === undefined) return null;
  return (
    <Button kind="tone" onClick={onOtherFolder} data-testid="permission-other-folder">
      {w(WINDOWS.act)}
    </Button>
  );
}

/** A refused row: where to change it, the desktop's note, and its ways on. */
function Refused({
  row,
  onSettings,
  onCheck,
  onChooseFolder,
  onNotNow,
}: { row: PermissionStepRow } & PermissionStepActs) {
  const w = useFirstLaunchWords();
  return (
    <>
      <p className="fl-perm-said" role="status" data-testid="permission-denied">
        <Say>{w("Not allowed.")}</Say>{" "}
        {row.id === "keychain" ? (
          <Say>{w("You can allow it in Settings, under Privacy, at any time.")}</Say>
        ) : (
          row.path !== undefined && (
            <Say>{w("You can allow it in {path}.", { path: row.path })}</Say>
          )
        )}
      </p>
      {/* the keychain's note names its items by service and account: never shown, only what they are for */}
      {row.id === "keychain" ? <KeychainItems note={row.note} /> : <Note note={row.note} />}
      {row.id === "files_documents" && (
        <Ways onSettings={onSettings} onCheck={onCheck} onChooseFolder={onChooseFolder} />
      )}
      {row.id === "keychain" && onNotNow !== undefined && (
        <div className="fl-perm-ways">
          <Button kind="quiet" size="small" onClick={onNotNow} data-testid="permission-not-now">
            {w("Not now")}
          </Button>
        </div>
      )}
    </>
  );
}

/** Windows Security's path without its own first step, which the words before it name. */
const underSecurity = (path: string) => path.replace(/^Windows Security\s*›\s*/, "");

/** What the keychain's refused items hold, by the services in the desktop's note ("service (account), …"):
 * "ai.clauseo.desktop.mcp" holds the keys of the lawyer's own connectors, a service with "connectors" in it Clauseo's
 * connectors' sign-ins, the rest Claude's and ChatGPT's sign-ins. */
const ITEMS: Record<string, FirstLaunchKey> = {
  plans: "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT.",
  connectors: "Without it, Clauseo can't read your saved sign-ins for connectors.",
  own: "Without it, Clauseo can't read the saved keys for your own connectors.",
  "plans connectors":
    "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT, or for connectors.",
  "plans own":
    "Without it, Clauseo can't read your saved sign-ins for Claude or ChatGPT, or the saved keys for your own connectors.",
  "connectors own":
    "Without it, Clauseo can't read your saved sign-ins for connectors, or the saved keys for your own connectors.",
  "plans connectors own":
    "Without it, Clauseo can't read your saved sign-ins for Claude, ChatGPT and connectors, or the saved keys for your own connectors.",
};

/** The kinds in the order the ITEMS keys name them. */
const KIND_ORDER = ["plans", "connectors", "own"] as const;

/** The kind of item a keychain service holds. */
const kindOf = (service: string) =>
  service.endsWith(".mcp") ? "own" : service.includes("connectors") ? "connectors" : "plans";

/** What the refused items hold, in plain words; the note itself never shows. */
function KeychainItems({ note }: { note: string | undefined }) {
  const w = useFirstLaunchWords();
  if (note === undefined) return null;
  const services = note
    .split(/,\s*/)
    .map((item) => item.replace(/\s*\(.*\)\s*$/, "").trim())
    .filter((service) => service !== "");
  const kinds = new Set(services.map(kindOf));
  const key = ITEMS[KIND_ORDER.filter((k) => kinds.has(k)).join(" ")];
  if (key === undefined) return null;
  return (
    <p className="fl-perm-note" data-testid="permission-note">
      <Say>{w(key)}</Say>
    </p>
  );
}

/** A row's quiet ways on, those the caller gives: Documents refused can also send new matters elsewhere. */
function Ways({
  onSettings,
  onCheck,
  onChooseFolder,
}: Pick<PermissionStepActs, "onSettings" | "onCheck" | "onChooseFolder">) {
  const w = useFirstLaunchWords();
  return (
    <div className="fl-perm-ways">
      {onSettings !== undefined && (
        <Button kind="quiet" size="small" onClick={onSettings} data-testid="permission-settings">
          {w("Open System Settings")}
        </Button>
      )}
      {onCheck !== undefined && <CheckAgain onCheck={onCheck} />}
      {onChooseFolder !== undefined && <FolderChoiceButton onChoose={onChooseFolder} />}
    </div>
  );
}

/** Reads the rows again. */
function CheckAgain({ onCheck }: { onCheck: () => void }) {
  const w = useFirstLaunchWords();
  return (
    <Button kind="quiet" size="small" onClick={onCheck} data-testid="permission-check">
      {w("Check again")}
    </Button>
  );
}
