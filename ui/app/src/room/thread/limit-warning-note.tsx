// The composer's warning as it shows (limits/limit-warning.tsx decides when): FieldNotice's quiet note, its first
// sentence the window, its second the reset, the host's `resets_at` in a <time>. In the thread's chunk (list.tsx
// re-exports it), where limit-note.tsx already is for the stop's note, so the first load carries none of it.
import { FieldNotice } from "@clauseo/components/clauseo/limit-note.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { Timestamp } from "../../transport/protocol.ts";
import { isoOf } from "./clock.ts";

export function WarningNote({
  lead,
  until,
}: {
  lead: string;
  until: { text: string; at: Timestamp } | null;
}) {
  return (
    <div className="contents" data-testid="limit-warning">
      <FieldNotice
        lead={lead}
        action={
          until === null ? null : (
            <time className="lim-field-lead" dateTime={isoOf(until.at)}>
              <Say>{until.text}</Say>
            </time>
          )
        }
      />
    </div>
  );
}
