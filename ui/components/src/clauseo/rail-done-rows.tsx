// The Done fold, open (U16b), loaded later: a matter's done conversations, each opening as any row does, with Mark as
// not done (the undo arrow) at its end; then what moves here, behind the info icon.
import { Button } from "../ui/button.tsx";
import { IconUndo } from "../icons/icons.tsx";
import { useDisplay } from "../lib/display.ts";
import { langOfText } from "../lib/script.ts";
import { twl } from "../lib/words-triage-late.ts";
import { RailInfo } from "./rail-info.tsx";
import { Say } from "./say.tsx";

export type DoneRow = { id: string; title: string; onOpen?: () => void; onNotDone: () => void };

export function DoneRows({ rows }: { rows: readonly DoneRow[] }) {
  const { lang } = useDisplay();
  const back = twl(lang, "Mark as not done");
  return (
    <>
      <ul className="room-conv-list">
        {rows.map((r) => (
          <li key={r.id} className="room-conv-item" data-testid="done-row">
            <button type="button" className="room-conv" title={r.title} onClick={r.onOpen}>
              <span className="room-conv-line">
                <span className="room-conv-title" lang={langOfText(r.title)}>
                  <Say>{r.title}</Say>
                </span>
              </span>
            </button>
            <span className="room-conv-acts">
              <Button
                kind="quiet"
                size="small"
                shape="icon"
                aria-label={`${back}, ${r.title}`}
                title={back}
                data-testid="mark-not-done"
                onClick={r.onNotDone}
              >
                <IconUndo size={16} />
              </Button>
            </span>
          </li>
        ))}
      </ul>
      <RailInfo
        words={twl(lang, "Read conversations with nothing waiting move here each morning.")}
      />
    </>
  );
}
