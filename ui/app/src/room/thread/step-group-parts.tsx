// One tool call's row in an open step group, live or from an older page.
import { useState } from "react";
import { StepRow } from "@clauseo/components/clauseo/step-group.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { EntryId, ToolCallEntry } from "../../transport/protocol.ts";
import { isoOf, stamp } from "./clock.ts";
import { doneOf, hoverOf, markOf, timeOf } from "./steps.ts";

/** One tool call's row in an open group: its time in the thread's clock words, today's as the row first showed. */
export function CallRow({ id, call, lang }: { id: EntryId; call: ToolCallEntry; lang: Lang }) {
  // "today" as the row first showed: no clock runs to move it
  const [now] = useState(Date.now);
  const at = timeOf(call);
  return (
    <StepRow
      id={id}
      toolCall={call.tool_call}
      status={call.status}
      kind={call.kind}
      text={doneOf(lang, call)}
      hover={hoverOf(call)}
      mark={markOf(lang, call)}
      background={call.background !== undefined}
      time={at === null ? null : { text: stamp(lang, at, now), iso: isoOf(at) }}
    />
  );
}
