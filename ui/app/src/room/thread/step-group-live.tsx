// A step group: it reads only its own entries, from an older page or the view store, the same array while none of them changes,
// and is memoized on its ids, so a flush of the answer's text renders it not at all. While its turn runs and nothing
// but a prompt for the lawyer stands after it, it is the running group, and its line says what is being done now.
import { memo, useCallback, useMemo, type ReactElement } from "react";
import { StepGroup, StepThought } from "@clauseo/components/clauseo/step-group.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { useStreamPart, useViewStore } from "../../store/hooks.ts";
import type { StreamView } from "../../store/view.ts";
import type { EntryId, StreamId, ViewEntry } from "../../transport/protocol.ts";
import { AnswerMeta } from "./answer-meta.tsx";
import { sameIds } from "./ids.ts";
import { turnRunning } from "./live.ts";
import { lineOf } from "./steps.ts";
import { RowOf } from "./row-of.tsx";
import { CallRow } from "./step-group-parts.tsx";

/** A group's entries from an older page or the view, the same array while each of them is the same object. */
function entriesPick(ids: readonly EntryId[], paged: ReadonlyMap<EntryId, ViewEntry>) {
  let last: (ViewEntry | undefined)[] = [];
  return (v: StreamView | undefined) => {
    const next = ids.map((id) => paged.get(id) ?? v?.entries.get(id));
    if (next.length === last.length && next.every((e, i) => e === last[i])) return last;
    last = next;
    return next;
  };
}

const pickRunning = (v: StreamView | undefined) => turnRunning(v?.header?.header?.status);

/** A live group. `tail`: nothing but a prompt for the lawyer stands after it in the latest answer, so while a turn
 * runs it is the running group. `meta`: it opens the answer, so its line stands on the answer's meta line, after the
 * name and the time of `step` (`latest`: the meta line the room measures). */
export const LiveStepGroup = memo(
  function LiveStepGroup({
    ids,
    stream,
    paged,
    tail,
    meta = false,
    step,
    latest = false,
  }: {
    ids: readonly EntryId[];
    stream: StreamId;
    paged: ReadonlyMap<EntryId, ViewEntry>;
    tail: boolean;
    meta?: boolean;
    step?: EntryId | undefined;
    latest?: boolean;
  }) {
    const { lang } = useDisplay();
    const store = useViewStore();
    const pick = useMemo(() => entriesPick(ids, paged), [ids, paged]);
    const entries = useStreamPart(store, stream, pick);
    const running = useStreamPart(store, stream, pickRunning);
    const live = tail && running;
    const words = useMemo(() => lineOf(lang, entries, live), [lang, entries, live]);
    const head = useCallback(
      (line: ReactElement) => (
        <AnswerMeta step={step} stream={stream} paged={paged} latest={latest}>
          {line}
        </AnswerMeta>
      ),
      [step, stream, paged, latest],
    );
    return (
      <StepGroup id={ids[0]!} {...words} live={live} head={meta ? head : undefined}>
        {ids.map((id, i) => {
          const call = entries[i]?.tool_call;
          if (call !== undefined) return <CallRow key={id} id={id} call={call} lang={lang} />;
          return (
            <StepThought key={id}>
              <RowOf id={id} stream={stream} paged={paged} sentNow={false} />
            </StepThought>
          );
        })}
      </StepGroup>
    );
  },
  (a, b) =>
    a.stream === b.stream &&
    a.tail === b.tail &&
    a.paged === b.paged &&
    a.meta === b.meta &&
    a.step === b.step &&
    a.latest === b.latest &&
    sameIds(a.ids, b.ids),
);
