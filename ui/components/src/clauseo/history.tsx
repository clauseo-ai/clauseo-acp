import { Popover } from "@base-ui/react/popover";
import { floating } from "../ui/recipes.ts";
import { cn } from "../lib/cn.ts";
import { useWords } from "../lib/words.ts";
import { useVersionWords } from "../lib/words-versions.ts";
import type { HistoryRow } from "./matter.ts";
import { Say } from "./say.tsx";

/** Versions: every step kept, with its time, in one place. A 380 px card under the Versions button, its right edge on
 * the Versions and Undo pair's, 8 px below and always inside the window. Rows put the actor first, newest first, each
 * with its time at the right; the foot says "Every change is kept." It drops in from 4 px above and 0.97 scale over
 * 160 ms and leaves in 90; Escape, a press outside or a second press closes it, and focus returns to Versions. */
export function HistoryPanel({
  rows,
  anchor,
}: {
  rows: readonly HistoryRow[];
  anchor: () => Element | null;
}) {
  const vw = useVersionWords();
  return (
    <Popover.Portal>
      <Popover.Positioner
        anchor={anchor}
        side="bottom"
        align="end"
        sideOffset={8}
        collisionPadding={8}
        positionMethod="fixed"
        className="z-30"
      >
        <Popover.Popup
          className={cn(floating({ width: "history" }), "shadow-e3 history-card")}
          aria-label={vw.v("Versions")}
        >
          <HistoryRows rows={rows} />
        </Popover.Popup>
      </Popover.Positioner>
    </Popover.Portal>
  );
}

/** What Versions holds: the rows, newest first, and the foot. */
function HistoryRows({ rows }: { rows: readonly HistoryRow[] }) {
  const w = useWords();
  return (
    <>
      <ol className="history-list">
        {rows.map((r, i) => (
          <li key={`${i}:${r.what}`} className="history-row">
            <span className="history-what">
              <Say>{r.what}</Say>
            </span>
            <time className="history-time">{r.at}</time>
          </li>
        ))}
      </ol>
      <p className="history-foot">{w.t("Every change is kept.")}</p>
    </>
  );
}
