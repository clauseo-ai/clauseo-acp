// One tool call's row in an open step group, live or from an older page. A finished call with no content but a raw
// output offers "Show the result" (U17): the press opens what came back under the row, its block loaded on the first
// press (tool-result/result-load.ts); an error read there turns the row's mark to Failed and its icon to the break.
import { memo, Suspense, useId, useState } from "react";
import { StepRow } from "@clauseo/components/clauseo/step-group.tsx";
import { Say } from "@clauseo/components/clauseo/say.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { tr } from "@clauseo/components/lib/words-tool-result.ts";
import type { EntryId, ToolCallEntry } from "../../transport/protocol.ts";
import { isoOf, stamp } from "./clock.ts";
import { doneOf, hoverOf, markOf, timeOf } from "./steps.ts";
import { eligible } from "./tool-result/eligible.ts";
import { ResultBlock } from "./tool-result/result-load.ts";

/** One tool call's row in an open group: its time in the thread's clock words, today's as the row first showed.
 * Memoized on its call, so another call's change doesn't render it, nor its open result. */
export const CallRow = memo(function CallRow({
  id,
  call,
  lang,
}: {
  id: EntryId;
  call: ToolCallEntry;
  lang: Lang;
}) {
  // "today" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const [open, setOpen] = useState(false);
  const [failed, setFailed] = useState(false);
  const [reading, setReading] = useState(false);
  const block = useId();
  const at = timeOf(call);
  const output = eligible(call) ? call.raw_output : undefined;
  return (
    <StepRow
      id={id}
      toolCall={call.tool_call}
      status={call.status}
      kind={call.kind}
      text={doneOf(lang, call)}
      hover={hoverOf(call)}
      mark={failed ? th(lang, "Failed") : markOf(lang, call)}
      background={call.background !== undefined}
      time={at === null ? null : { text: stamp(lang, at, now), iso: isoOf(at) }}
      failed={failed}
      resultOpen={output === undefined ? undefined : open}
      result={
        output === undefined ? undefined : (
          <>
            <button
              type="button"
              className="step-row-press"
              aria-expanded={open}
              aria-controls={block}
              disabled={reading}
              onClick={() => setOpen((o) => !o)}
            >
              <Say>
                {tr(
                  lang,
                  reading ? "Reading the result." : open ? "Hide the result" : "Show the result",
                )}
              </Say>
            </button>
            {open && (
              <Suspense fallback={null}>
                <ResultBlock
                  id={block}
                  output={output}
                  lang={lang}
                  onFailed={setFailed}
                  onReading={setReading}
                />
              </Suspense>
            )}
          </>
        )
      }
    />
  );
});
