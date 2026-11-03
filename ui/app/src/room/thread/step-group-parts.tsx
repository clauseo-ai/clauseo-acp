// One tool call's row in an open step group, live or from an older page.
import { StepRow } from "@clauseo/components/clauseo/step-group.tsx";
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { EntryId, ToolCallEntry } from "../../transport/protocol.ts";
import { doneOf, hoverOf, markOf } from "./steps.ts";

/** One tool call's row in an open group. */
export function CallRow({ id, call, lang }: { id: EntryId; call: ToolCallEntry; lang: Lang }) {
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
    />
  );
}
