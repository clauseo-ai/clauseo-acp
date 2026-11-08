// Design B's in-app repair: where the set-up program's admin copy is missing on a personal install
// that can fetch its own installer (display_info's repair_available: Windows, per user, a release or
// test build with its updater key), the main window's card and first-run step offer "Download and
// run the installer again" (desktop's repair_install). The press waits on "Downloading the
// installer…", then "Windows will ask for permission. Choose Yes."; then the desktop closes every
// window, starts the installer and quits, so nothing more is drawn. A refusal says its code's words,
// and Get the installer (installer.ts) stays beside it, with the button offered again. A firm's
// install, a build without the repair and the settings window keep today's repair as it is. While it
// downloads, Cancel beside the button stops it (desktop's repair_cancel), and the card goes back to
// idle with no line. After the installer, display_info's `repair` brings the lawyer back once: to the
// card while the copy is still missing ("The installer didn't finish…"), or to the set-up once it is
// there; a launch that failed, with the window built again, says its code's words. The run lives in
// the window's protection store (store/protection.ts), so the note and the step share it.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { protectionErrorWords, pw } from "@clauseo/components/lib/words-protection.ts";
import type { ProtectionState, ProtectionStore, RepairState } from "../../store/protection.ts";
import { toHostError, type HostTransport } from "../../transport/host-transport.ts";
import { firmInstall, needsInstaller } from "./blocked.ts";
import type { Cause } from "./causes.ts";

const setRepair = (store: ProtectionStore, patch: Partial<RepairState>): void =>
  store.set({ repair: { ...store.state().repair, ...patch } });

/** Whether the card offers "Download and run the installer again": the admin copy missing, a
 * personal install (not a firm's), the desktop says the repair runs here, and the transport carries
 * it. */
export const repairOffered = (
  transport: HostTransport,
  state: ProtectionState,
  cause: Cause | null,
): boolean =>
  transport.repairInstall !== undefined &&
  state.repair.available &&
  needsInstaller(cause) &&
  !firmInstall(state.view, state.managedBy);

/** Runs the repair once: a press while one runs sends nothing. Each step lands in the store; a
 * refusal leaves its code. Once the installer starts the window is gone and nothing answers. A
 * cancelled run ("cancelled", the card's Cancel) goes back to idle with no line at all. */
export async function runRepair(transport: HostTransport, store: ProtectionStore): Promise<void> {
  const send = transport.repairInstall;
  if (send === undefined || store.state().repair.step !== null) return;
  setRepair(store, { step: "starting", failed: null });
  try {
    await send.call(transport, (step) => setRepair(store, { step }));
    setRepair(store, { step: null, cancelling: false });
  } catch (thrown) {
    setRepair(store, { step: null, cancelling: false, failed: toHostError(thrown).code });
  }
}

/** Stops the download (the card's Cancel). Only while it downloads, and once: a press while the
 * answer is due sends nothing. A refused cancel (nothing downloads) leaves the run as it is. */
export async function cancelRepair(
  transport: HostTransport,
  store: ProtectionStore,
): Promise<void> {
  const send = transport.repairCancel;
  const { step, cancelling } = store.state().repair;
  if (send === undefined || cancelling || (step !== "starting" && step !== "downloading")) return;
  setRepair(store, { cancelling: true });
  try {
    await send.call(transport);
  } catch {
    setRepair(store, { cancelling: false });
  }
}

/** What the card or the step shows of the repair. */
export type RepairView = {
  /** "Download and run the installer again" shows, with this press; undefined for none. */
  onRepair: (() => void) | undefined;
  /** "Cancel" beside the busy button while the download runs (not while Windows' prompt is up);
   * undefined for none. */
  onCancel: (() => void) | undefined;
  /** The waiting line while it runs (the button disabled); null when none runs. */
  wait: string | null;
  /** The last refusal in its code's words; null for none. */
  failed: string | null;
  /** The line after a start that follows an installer which didn't set the copy up; null for none. */
  returned: string | null;
};

/** The repair's view for a cause, from the store's state as the caller read it (so a memoised
 * render follows it): nothing at all unless the admin copy is missing. */
export function repairView(
  lang: Lang,
  transport: HostTransport,
  store: ProtectionStore,
  state: ProtectionState,
  cause: Cause | null,
): RepairView {
  const { step, failed, note, cancelling } = state.repair;
  const missing = needsInstaller(cause);
  const downloading = step === "starting" || step === "downloading";
  return {
    onRepair: repairOffered(transport, state, cause)
      ? () => void runRepair(transport, store)
      : undefined,
    onCancel:
      transport.repairCancel !== undefined && downloading && !cancelling
        ? () => void cancelRepair(transport, store)
        : undefined,
    wait:
      step === null
        ? null
        : step === "launching"
          ? pw(lang, "Windows will ask for permission. Choose Yes.")
          : pw(lang, "Downloading the installer…"),
    failed: missing && failed !== null ? protectionErrorWords(lang, failed) : null,
    returned:
      missing && note === "returned" && step === null && failed === null
        ? pw(
            lang,
            "The installer didn't finish setting up the Safe workspace. Try again, or choose Get the installer.",
          )
        : null,
  };
}

/** Whether the room should bring the lawyer to this cause's card now, once per run after the
 * installer: only while the copy is still missing (declined, failed or aborted, or the launch
 * failed). Once the copy is there nothing opens: the installer's elevated `--adopt` runs the
 * Safe workspace's set-up itself (its step 8), so the protection shows where it always does, and
 * a set-up that failed there offers Set up on the usual card. Not while the first run's step
 * stands. */
export const landingDue = (state: ProtectionState, cause: Cause | null): boolean =>
  state.repair.note !== null &&
  !state.repair.landed &&
  state.step !== "ask" &&
  needsInstaller(cause);

/** Marks this run's landing done. */
export const markLanded = (store: ProtectionStore): void => setRepair(store, { landed: true });
