// The first screen's permissions step (issue 5): what macOS will ask Clauseo for, asked here first, each with its
// reason, so no system notice stops the lawyer in the middle of a matter. On the Mac at most two rows, in order: the
// Documents folder (where the matters folder sits inside it), then the keychain (where a read of Clauseo's own items
// would prompt). Only the first row not answered yet has its Allow; macOS's own dialog comes only after that press, and
// the row waits for the lawyer's answer there. An allowed row shows its tick and is never asked again; a refused one
// says where to change it, in the system's words the desktop gives. On Windows nothing is asked: where Controlled
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

/** One row as the step draws it. "ask" is the row whose turn it is; "later" waits for the row before it. */
export type PermissionStepRow = {
  id: "files_documents" | "keychain" | "controlled_folder_access";
  shows: "ask" | "later" | "waiting" | "granted" | "denied" | "failed" | "blocking" | "unknown";
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
  /** Where the host can set a matters folder (Windows): the picker, then the command; ahead of onOtherFolder. */
  onChooseFolder?: (() => Promise<FolderChoice | null>) | undefined;
};

/** The step, at the head of the first screen's question. */
export function PermissionsStep({
  rows,
  platform,
  ...acts
}: { rows: readonly PermissionStepRow[]; platform: "mac" | "windows" } & PermissionStepActs) {
  const w = useFirstLaunchWords();
  const heading = useId();
  return (
    <section
      className="fl-perms"
      aria-labelledby={heading}
      data-testid="permissions-step"
      data-platform={platform}
    >
      <h2 id={heading} className="fl-perms-head">
        <Say>{w("Before your first matter")}</Say>
      </h2>
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
        {(shows === "ask" || shows === "failed") && (
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
      {shows === "denied" && <Refused row={row} {...acts} />}
      {(shows === "blocking" || shows === "unknown") && (
        <>
          {/* the desktop's detail, as given: where the setting lives, and its note */}
          <Note note={row.path} testId="permission-path" />
          <Note note={row.note} />
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
function Refused({ row, onSettings, onCheck }: { row: PermissionStepRow } & PermissionStepActs) {
  const w = useFirstLaunchWords();
  return (
    <>
      <p className="fl-perm-said" role="status" data-testid="permission-denied">
        <Say>{w("Not allowed.")}</Say>{" "}
        {row.id === "keychain" ? (
          <Say>{w("macOS asks again the next time Clauseo opens. Choose Always Allow then.")}</Say>
        ) : (
          row.path !== undefined && (
            <Say>{w("You can allow it in {path}.", { path: row.path })}</Say>
          )
        )}
      </p>
      <Note note={row.note} />
      {row.id === "files_documents" && <Ways onSettings={onSettings} onCheck={onCheck} />}
    </>
  );
}

/** A row's quiet ways on, those the caller gives. */
function Ways({ onSettings, onCheck }: Pick<PermissionStepActs, "onSettings" | "onCheck">) {
  const w = useFirstLaunchWords();
  return (
    <div className="fl-perm-ways">
      {onSettings !== undefined && (
        <Button kind="quiet" size="small" onClick={onSettings} data-testid="permission-settings">
          {w("Open System Settings")}
        </Button>
      )}
      {onCheck !== undefined && <CheckAgain onCheck={onCheck} />}
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
