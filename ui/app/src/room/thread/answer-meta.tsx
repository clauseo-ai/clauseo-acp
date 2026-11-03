// An answer's meta line: "Clauseo", and its turn's time once the step is in, with the answer's first step group on it
// as the design's chip when the work came before the answer's first words (step-groups.tsx). It reads only the step's
// time, a number, so a flush of the answer's text renders it not at all.
import { useState, type ReactNode } from "react";
import { Who } from "@clauseo/components/clauseo/answer.tsx";
import { useRoom } from "@clauseo/components/clauseo/room-scope.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { stamp } from "./clock.ts";
import { useTimeOf } from "./time-of.ts";

/** The meta line that opens an answer. `step`: the turn's step, for its time; `latest`: the thread's latest exchange,
 * whose meta line the room measures; `children`: the chip after the name and the time. */
export function AnswerMeta({
  step,
  stream,
  paged,
  latest,
  children,
}: {
  step: EntryId | undefined;
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
  return (
    <div className="meta meta-live" ref={latest ? nodes.slot("meta") : undefined}>
      <Who name={w.t("Clauseo")} time={ended === null ? undefined : stamp(w.lang, ended, now)} />
      {children}
    </div>
  );
}
