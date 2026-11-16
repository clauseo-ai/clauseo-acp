// Settings › Privacy on Windows: what Clauseo can't keep apart there (UC-820), then the Mac advice (UC-850), the room's
// one-time notice's words (room/windows-line/), readable here for good. Settings › Privacy ships at launch while the
// Safe workspace is parked, so the line stands here, not tied to it. Its own chunk, loaded only on Windows (privacy.tsx).
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { pw, WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-protection.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { Group } from "./parts.tsx";

export function PrivacyWindowsLine() {
  const w = useSettingsWords();
  const { lang } = useDisplay();
  return (
    <Group plate={false}>
      <p className="set-note" data-testid="privacy-windows-line">
        {w.say(pw(lang, WINDOWS_LINE))}
        {/* UC-850, Rohan's call: its own sentence, to go alone */}{" "}
        {w.say(pw(lang, WINDOWS_STRICT))}
      </p>
    </Group>
  );
}
