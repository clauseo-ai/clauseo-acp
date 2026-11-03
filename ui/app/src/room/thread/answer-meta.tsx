// An answer's meta line: "Clauseo", and its turn's time once the step is in, with the answer's first step group on it
// as the design's chip when the work came before the answer's first words (step-groups.tsx). While the answer is live
// it shows when the answer started, where the view says so honestly (startOf), so the chip after it stands where it
// will once the turn ends. It reads only times and whether the turn waits in a queue, so a flush of the answer's text
// renders it not at all.
import { useState, type ReactNode } from "react";
import { Who } from "@clauseo/components/clauseo/answer.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { pickQueued, startOf, useTimeOf } from "./time-of.ts";

/** The meta line that opens an answer. `step`: the turn's step, for its time; `asked` and `before`: its question and
 * the step of the exchange before, for when it started (startOf), `asked` left out where the turn was seen waiting
 * behind another conversation (LiveExchange keeps that, as this line remounts when the answer's first row comes); `latest`: the thread's latest exchange, whose meta
 * line the room measures; `children`: the chip after the name and the time. */
export function AnswerMeta({
  step,
  asked,
  before,
  stream,
  paged,
  latest,
  children,
}: {
  step: EntryId | undefined;
  asked?: EntryId | undefined;
  before?: EntryId | undefined;
  stream: StreamId;
  paged: ReadonlyMap<EntryId, ViewEntry>;
  latest: boolean;
  children?: ReactNode;
}) {
  const w = useWords();
  const { nodes } = useRoom();
  // "today" as the line first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const ended = useTimeOf(step, stream, paged);
  const queued = useStreamPart(useViewStore(), stream, pickQueued);
  const start = startOf(
    useTimeOf(asked, stream, paged),
    useTimeOf(before, stream, paged),
    ended === null && queued,
  );
  const started = start === null ? null : stamp(w.lang, start, now);
  return (
    <div className="meta meta-live" ref={latest ? nodes.slot("meta") : undefined}>
      {/* live, the time it started; at the end, the time it ended, in a place kept at least as wide as the start's
          (its digits all one width), so a shorter end time can't pull the chip left */}
      <Who
        name={w.t("Clauseo")}
        time={ended !== null ? stamp(w.lang, ended, now) : (started ?? undefined)}
        held={started === null ? undefined : [started]}
      />
      {children}
    </div>
  );
}
