// An answer's rows with its step groups (steps.ts): every run of tool calls and thoughts between two of the agent's
// texts folds into one step group (step-group-live.tsx), the rest stand as their own rows (rows.tsx). Its rows are live
// ones or an older page's (`paged`): a turn can start on a page and go on in the window. The list groups by exchange,
// so a group never spans two. The segments are the same array while the answer's ids are, so a flush of its text
// renders nothing here.
import { memo, useMemo } from "react";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { sameIds } from "./ids.ts";
import { RowOf } from "./row-of.tsx";
import { LiveStepGroup } from "./step-group-live.tsx";
import { segmentsPick, type Segment } from "./steps.ts";

/** A prompt for the lawyer: it stands after the running group without ending it. */
const PROMPT = /^(permission|elicitation):/;

/** Whether segment `i` is a group with nothing after it but prompts for the lawyer. */
const isTail = (segments: readonly Segment[], i: number) =>
  segments.slice(i + 1).every((s) => s.row !== undefined && PROMPT.test(s.row));

/** An answer's rows, its groups folded. `latest`: the thread's latest exchange, whose last group can be running.
 * `sentNow`: the step of a turn a steer sent now stopped. */
export const LiveAnswerRows = memo(
  function LiveAnswerRows({
    ids,
    stream,
    paged,
    sentNow,
    latest,
  }: {
    ids: readonly EntryId[];
    stream: StreamId;
    paged: ReadonlyMap<EntryId, ViewEntry>;
    sentNow: EntryId | null;
    latest: boolean;
  }) {
    const pick = useMemo(() => segmentsPick(ids, paged), [ids, paged]);
    const segments = useStreamPart(useViewStore(), stream, pick);
    return segments.map((s, i) =>
      s.row !== undefined ? (
        <RowOf key={s.row} id={s.row} stream={stream} paged={paged} sentNow={s.row === sentNow} />
      ) : (
        <LiveStepGroup
          key={s.group[0]}
          ids={s.group}
          stream={stream}
          paged={paged}
          tail={latest && isTail(segments, i)}
        />
      ),
    );
  },
  (a, b) =>
    a.stream === b.stream &&
    a.latest === b.latest &&
    a.sentNow === b.sentNow &&
    a.paged === b.paged &&
    sameIds(a.ids, b.ids),
);
