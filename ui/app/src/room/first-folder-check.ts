// The first screen's Open the folder, before and after the create (folder rights, FR7 to FR10, FR13, FR14): the
// folder picked is asked about as New matter's form asks (folder-check.tsx), and a network drive or a drive without
// file permissions holds the pick for the step (first-folder-step.tsx). Anything else, a refusal or a check that
// couldn't answer included, says nothing here: the create goes ahead and the host checks again. And a folder a matter
// on the list already uses opens that matter, refused (matter.folder_is_matter) or answered with it (MatterCreated).
import type { FolderRefId, MatterId } from "../transport/protocol.ts";
import type { Warned } from "./folder-warning.tsx";
import type { Matters } from "./matters.ts";

/** How long the pick waits for the check, as New matter's Create does (folder-check.tsx's CHECK_WAIT_MS). */
const CHECK_WAIT_MS = 5000;

/** The warnings this step gives: a drive's. A folder whose name has a backslash gets Locate's step first (FR30). */
export type DriveWarned = Exclude<Warned, "backslash">;

/** A pick held for the step: the folder as it shows, its warning, and the step's two ways on. */
export type HeldFolder = {
  display: string;
  kind: DriveWarned;
  /** Choose another folder: the picker again. */
  again: () => Promise<void>;
  /** Open anyway: the folder made a matter. */
  anyway: () => Promise<void>;
};

/** The warning for a folder picked, or null: fine, refused, or no answer within the wait. */
export async function warningOf(
  matters: Matters,
  folder: FolderRefId,
): Promise<DriveWarned | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const asked = matters.folderSupport({ existing_folder: { folder } }).then(
    (support) => (support === "network" || support === "no_permissions" ? support : null),
    () => null,
  );
  try {
    return await Promise.race([
      asked,
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), CHECK_WAIT_MS);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

/** The matter on the list whose folder the refusal says it is (matter.folder_is_matter), the list read again first;
 * null for any other refusal, or a matter the list doesn't have. */
export async function usedBy(
  matters: Matters,
  refused: { code: string; matter?: MatterId | undefined },
): Promise<MatterId | null> {
  if (refused.code !== "matter.folder_is_matter" || refused.matter === undefined) return null;
  await matters.refresh();
  return matters.view().matters.some((m) => m.matter === refused.matter) ? refused.matter : null;
}
