// An answer's rows with its step groups (steps.ts): every run of tool calls and thoughts between two of the agent's
// texts folds into one step group (step-group-live.tsx), the rest stand as their own rows (rows.tsx). Its rows are live
// ones or an older page's (`paged`): a turn can start on a page and go on in the window. The list groups by exchange,
// so a group never spans two. The segments are the same array while the answer's ids are, so a flush of its text
// renders nothing here.
import { Fragment, memo, useMemo, type ReactNode } from "react";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { AnswerMeta } from "./answer-meta.tsx";
import { sameIds } from "./ids.ts";
import { RowOf } from "./row-of.tsx";
import { LiveStepGroup } from "./step-group-live.tsx";
import { segmentsPick, type Segment } from "./steps.ts";

/** A turn's step row: how it ended. */
const STEP = "step:";

/** A prompt for the lawyer: it stands after the running group without ending it. */
const PROMPT = /^(permission|elicitation):/;

/** Whether segment `i` is a group with nothing after it but prompts for the lawyer. */
const isTail = (segments: readonly Segment[], i: number) =>
  segments.slice(i + 1).every((s) => s.row !== undefined && PROMPT.test(s.row));

/** An answer's rows, its groups folded, under its meta line. `latest`: the thread's latest exchange, whose last group
 * can be running. `sentNow`: the step of a turn a steer sent now stopped. `step`: the turn's step, for the meta
 * line's time. A group that opens the answer, before its first words, stands on the meta line as the design's chip;
 * a group between two texts stands on its own line where it falls. `sources`: the turn's web sources (sources.tsx), after
 * the answer's words and before its step row (how it ended, the change note). */
export const LiveAnswerRows = memo(
  function LiveAnswerRows({
    ids,
    stream,
    paged,
    sentNow,
    latest,
    step,
    asked,
    before,
    sources,
  }: {
    ids: readonly EntryId[];
    step: EntryId | undefined;
    /** The exchange's question and the step of the exchange before, for the meta line's time while live. */
    asked?: EntryId | undefined;
    before?: EntryId | undefined;
    stream: StreamId;
    paged: ReadonlyMap<EntryId, ViewEntry>;
    sentNow: EntryId | null;
    latest: boolean;
    sources?: ReactNode;
  }) {
    const pick = useMemo(() => segmentsPick(ids, paged), [ids, paged]);
    const segments = useStreamPart(useViewStore(), stream, pick);
    const chip = segments[0]?.group !== undefined;
    const rows = segments.map((s, i) =>
      s.row !== undefined ? (
        <RowOf key={s.row} id={s.row} stream={stream} paged={paged} sentNow={s.row === sentNow} />
      ) : (
        <LiveStepGroup
          key={s.group[0]}
          ids={s.group}
          stream={stream}
          paged={paged}
          tail={latest && isTail(segments, i)}
          meta={i === 0}
          step={step}
          asked={asked}
          before={before}
          latest={latest}
        />
      ),
    );
    if (sources !== undefined) {
      const at = segments.findIndex((s) => s.row?.startsWith(STEP) === true);
      rows.splice(at < 0 ? rows.length : at, 0, <Fragment key="sources">{sources}</Fragment>);
    }
    if (chip) return rows;
    return (
      <>
        <AnswerMeta
          step={step}
          asked={asked}
          before={before}
          stream={stream}
          paged={paged}
          latest={latest}
        />
        {rows}
      </>
    );
  },
  (a, b) =>
    a.stream === b.stream &&
    a.latest === b.latest &&
    a.sentNow === b.sentNow &&
    a.step === b.step &&
    a.asked === b.asked &&
    a.before === b.before &&
    a.paged === b.paged &&
    sameIds(a.ids, b.ids),
);
