// The "Test build" label: a build made with the desktop's e2e feature says so, at the rail's foot and in Settings'
// About beside the version. A muted tag, in the page's language: not a control, not focusable, not an alert.
import type { Lang } from "../lib/display.ts";
import { useDisplay } from "../lib/display.ts";
import { ui } from "../lib/words-ui.ts";

/** The label. `lang` is the page's language, unless a caller shows it in another (the review's cell does). */
export function TestBuildTag({ lang }: { lang?: Lang }) {
  const display = useDisplay();
  return (
    <span className="test-build" data-testid="test-build">
      {ui(lang ?? display.lang, "Test build")}
    </span>
  );
}
