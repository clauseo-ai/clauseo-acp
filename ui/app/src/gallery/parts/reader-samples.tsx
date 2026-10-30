// The reader's samples that hold state, so a press shows the part's next state: the draft's changes settle as the lawyer
// accepts or rejects them, the lookup guard folds once answered, and Word's lock dialog opens on a press, never on load.
import { useState } from "react";
import { Button } from "@clauseo/components/ui/button.tsx";
import { useReaderWords } from "@clauseo/components/lib/words-reader.ts";
import { DraftPane } from "@clauseo/components/clauseo/draft-pane.tsx";
import { WordLockDialog } from "@clauseo/components/clauseo/draft-review.tsx";
import type { ChangeId, Review } from "@clauseo/components/clauseo/judgments.ts";
import { LookupGuard, type GuardState } from "@clauseo/components/clauseo/ik.tsx";
import { DRAFT, NOW } from "./reader-data.ts";

const noop = () => {};

/** The draft's changes and citations beside it, with the tab and each change's Accept, Reject and Undo held here. */
export function DraftPaneSample({
  start,
  tab: first,
}: {
  start: Review;
  tab: "changes" | "citations";
}) {
  const [review, setReview] = useState<Review>(start);
  const [tab, setTab] = useState(first);
  const act = (ids: readonly ChangeId[], how: "accept" | "reject" | "undo") =>
    setReview((r) => {
      const changes = { ...r.changes };
      for (const id of ids)
        changes[id] =
          how === "undo"
            ? { state: "pending", at: null }
            : { state: how === "accept" ? "accepted" : "rejected", at: NOW };
      return { ...r, changes };
    });
  return (
    <DraftPane
      draft={DRAFT}
      review={review}
      tab={tab}
      now={NOW}
      onTab={setTab}
      onAct={act}
      onCheck={noop}
      onFocus={(id) => setReview((r) => ({ ...r, focus: id }))}
    />
  );
}

/** Accept on a draft that Word holds: the lock dialog opens over the page and closes on Cancel or Try again. */
export function WordLockSample() {
  const w = useReaderWords();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button kind="tone" size="small" onClick={() => setOpen(true)}>
        {w.r("Accept")}
      </Button>
      <WordLockDialog
        file={DRAFT.file}
        open={open}
        act="accept"
        onClose={() => setOpen(false)}
        onTryAgain={() => setOpen(false)}
      />
    </>
  );
}

/** The guard after 300 lookups, asking: Continue or Answer with what's found folds it into its line. */
export function LookupSample({
  so,
  expected,
}: {
  so: { searches: number; read: number; fit: number };
  expected: string;
}) {
  const [state, setState] = useState<GuardState>("pending");
  return (
    <LookupGuard
      state={state}
      at="4:39 pm"
      so={so}
      expected={expected}
      onAnswer={(a) => setState(a)}
    />
  );
}
