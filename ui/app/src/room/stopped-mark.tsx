// Where an answer stopped because Claude stopped: the design's quiet mark under the answer so far, "The answer stopped
// here at 4:31 pm", in the step row's place for its code's sentence. Only agent.exited has a true time (thread-break.tsx,
// TIMED); every other failure keeps its code's words. It reads nothing from the store: the step's own time, and "now"
// as the row first showed, so no clock runs and a streaming flush never renders it.
import { memo, useState } from "react";
import { StopMark } from "@clauseo/components/clauseo/limit-note.tsx";
import { brk } from "@clauseo/components/lib/words-breaks.ts";
import { useWords } from "@clauseo/components/lib/words.ts";
import type { StepEntry } from "../transport/protocol.ts";
// the break's working lines ride in the thread's chunk with the mark: imported for the hand-over it makes as it runs
import "./break-lines.ts";
import { TIMED } from "./thread-break.ts";
import { isoOf, stamp } from "./thread/clock.ts";

/** The mark, for a step thread-break.ts's `takesMark` says yes to. */
export const StoppedMark = memo(function StoppedMark({ ts }: { ts: StepEntry["ts"] }) {
  const w = useWords();
  const [now] = useState(Date.now);
  return (
    <p className="text-small text-quiet" data-testid="stopped-here" data-code={TIMED}>
      <time dateTime={isoOf(ts)}>
        <StopMark>
          {brk(w.lang, "The answer stopped here at {time}", { time: stamp(w.lang, ts, now) })}
        </StopMark>
      </time>
    </p>
  );
});
