// Settings › Privacy on Windows: what Clauseo can't keep apart there (UC-820), then the Mac advice (UC-850), the room's
// one-time notice's words (room/windows-line/), readable here for good. Settings › Privacy ships at launch while the
// Safe workspace is parked, so the line stands here, not tied to it. Part of the settings window's first load, its words
// (words-windows-line.ts, a few hundred bytes) with it: as a lazy chunk of its own, it made the build split a chunk that
// the Connectors and Plans chunks import and other chunks import too, against vite-config.test.ts's rule.
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { wl, WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-windows-line.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { Group } from "./parts.tsx";

export function PrivacyWindowsLine() {
  const w = useSettingsWords();
  const { lang } = useDisplay();
  return (
    <Group plate={false}>
      <p className="set-note" data-testid="privacy-windows-line">
        {w.say(wl(lang, WINDOWS_LINE))}
        {/* UC-850, Rohan's call: its own sentence, to go alone */}{" "}
        {w.say(wl(lang, WINDOWS_STRICT))}
      </p>
    </Group>
  );
}
