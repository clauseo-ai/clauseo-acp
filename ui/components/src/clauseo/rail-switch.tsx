// Matters / Recent: two segments under Find that turn the rail's list between the matters in their groups and every
// conversation, newest first (rail-recent.tsx). The rail hides it while Find has the focus or words, when the list is
// Find's. Each segment is a toggle button, the one on pressed.
import { mt } from "../lib/words-matters.ts";
import { useWords } from "../lib/words.ts";

export type RailView = "matters" | "recent";

export function RailSwitch({
  view,
  onView,
  testId,
}: {
  view: RailView;
  onView: (view: RailView) => void;
  /** Optional: a test id on the group; each segment carries `${testId}-matters` and `${testId}-recent`. */
  testId?: string;
}) {
  const w = useWords();
  const segment = (v: RailView, label: string) => (
    <button
      type="button"
      aria-pressed={view === v}
      data-testid={testId === undefined ? undefined : `${testId}-${v}`}
      onClick={() => onView(v)}
    >
      <span className="deva-nudge">{label}</span>
    </button>
  );
  return (
    <div className="room-switch" role="group" data-testid={testId}>
      {segment("matters", w.t("Matters"))}
      {segment("recent", mt(w.lang, "Recent"))}
    </div>
  );
}
