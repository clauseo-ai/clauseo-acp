// The line that says the folder picker is on its way, under the folder line of New matter's form, on the first
// screen and by a moved folder's Locate. The caller shows it only once a pick has run long enough (picker-wait.ts).
// Muted, a status a screen reader hears; it takes no focus and no spinner.
import { useDisplay } from "../lib/display.ts";
import { mt } from "../lib/words-matters.ts";
import { Say } from "./say.tsx";

export function PickerLine() {
  const { lang } = useDisplay();
  return (
    <p role="status" className="text-small text-muted" data-testid="picker-opening">
      <Say>{mt(lang, "Opening the folder picker…")}</Say>
    </p>
  );
}
