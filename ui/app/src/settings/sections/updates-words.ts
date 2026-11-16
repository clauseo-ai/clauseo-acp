// R23: Settings' Updates in words: who manages updates where Clauseo doesn't, and where the updater stands now.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { uw, type UpdateKey } from "@clauseo/components/lib/words-update.ts";
import { updaterOf } from "../../store/update.ts";
import type { ManagedBy, UpdateView } from "../../transport/protocol.ts";

/** Who manages updates, in one line each. */
export const MANAGED_WORDS: Record<ManagedBy, UpdateKey> = {
  policy: "Your firm manages updates for Clauseo.",
  per_machine:
    "Clauseo is installed for everyone on this computer, so it doesn't update itself. To update, run the new installer from an administrator account.",
  read_only_location:
    "Clauseo can't update itself from where it's installed. To update, install the new version over this one.",
  package: "Updates come through the package manager that installed Clauseo.",
  unavailable:
    "This version of Clauseo doesn't update itself. To update, install the new version over this one. Your matters and settings stay.",
};

/** The policy key that locks the mode. */
export const MODE_KEY = "update.mode";

/** Where the updater stands, in words; null where nothing needs saying (idle before any check). */
export function standingWords(lang: Lang, view: UpdateView): string | null {
  const { name } = updaterOf(view.state);
  const s = view.state;
  switch (name) {
    case "checking":
      return uw(lang, "Checking for updates…");
    case "downloading":
      return uw(lang, "Downloading the update…");
    case "ready":
    case "scheduled":
      return uw(lang, "An update is ready. Restart to update from the main window.");
    case "installing":
      return uw(lang, "Installing the update…");
    case "failed":
      return typeof s === "object" && s.failed?.stage === "install"
        ? errorWords(lang, "update.failed")
        : uw(lang, "Clauseo couldn't check for updates. Try again.");
    case "idle":
      return view.checked_at === undefined ? null : uw(lang, "Clauseo is up to date.");
    default:
      return null;
  }
}
