// The Windows line's notice (UC-820; windows-line.tsx decides when): a quiet status line at the top of the work
// column, as the Smart App Control note's, never a dialog. The line is long, so it wraps within the column's measure;
// it stands at the top of every conversation until "Got it" puts it away for good ("Got it" is New matter's
// synced-folder notice's words, Hindi and all). The same words stay readable in Settings › Privacy on Windows.
import { Say } from "@clauseo/components/clauseo/say.tsx";
import { IconAbout } from "@clauseo/components/icons/icons.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { pw, WINDOWS_LINE, WINDOWS_STRICT } from "@clauseo/components/lib/words-protection.ts";
import { sc } from "@clauseo/components/lib/words-scan.ts";
import { Button } from "@clauseo/components/ui/button.tsx";

export function WindowsLineNote({ onGotIt }: { onGotIt: () => void }) {
  const { lang } = useDisplay();
  return (
    <div
      className="mx-auto box-content max-w-measure px-thread-pad pt-s4"
      data-testid="windows-line"
    >
      <div className="flex items-start gap-s2 text-small text-muted" role="status">
        <span className="flex-none pt-s1" aria-hidden="true">
          <IconAbout size={16} />
        </span>
        <p className="min-w-0 flex-1">
          <Say>{pw(lang, WINDOWS_LINE)}</Say>
          {/* UC-850, Rohan's call: its own sentence, to go alone */}{" "}
          <Say>{pw(lang, WINDOWS_STRICT)}</Say>
        </p>
        <Button kind="quiet" size="small" data-testid="windows-line-got-it" onClick={onGotIt}>
          <span className="deva-nudge">{sc(lang, "Got it")}</span>
        </Button>
      </div>
    </div>
  );
}
