// Open Privacy settings under a turn macOS kept out of its matter's folder (matter.folder_permission), on the Mac
// only: the desktop's open_privacy_settings opens System Settings at Files and Folders, where the lawyer turns on
// access for Clauseo. The line under the button says so. Windows and Linux have no such pane, so no button.
import { use } from "react";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { errorSentence, PRIVACY_KEYS } from "@clauseo/components/lib/words-errors.ts";
import { Button } from "@clauseo/components/ui/button.tsx";
import { SourcesTransport } from "./sources.ts";

/** The code whose stop offers the button. */
const FOLDER_PERMISSION = "matter.folder_permission";

/** The button and its line for a stop's `code`; nothing for another code, off the Mac, or outside the thread. */
export function PrivacySettings({ code }: { code: string }) {
  const { lang, platform } = useDisplay();
  const transport = use(SourcesTransport);
  if (code !== FOLDER_PERMISSION || platform !== "mac" || transport === null) return null;
  // a refusal changes nothing on screen: the stop's own words still say what is wrong
  const open = () => void transport.openPrivacySettings().catch(() => {});
  return (
    <div className="mt-(--s2) flex flex-col items-start gap-(--s1)">
      <Button kind="quiet" size="small" onClick={open} data-testid="open-privacy-settings">
        <span className="deva-nudge">{errorSentence(lang, PRIVACY_KEYS.button)}</span>
      </Button>
      <p className="text-small text-quiet" data-testid="privacy-settings-line">
        <Say>{errorSentence(lang, PRIVACY_KEYS.line)}</Say>
      </p>
    </div>
  );
}
