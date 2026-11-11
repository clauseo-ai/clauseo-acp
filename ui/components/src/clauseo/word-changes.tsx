// A Word document's own tracked changes, in the wall's place: each with its author as Word recorded it, its time,
// what it does and its words, with Accept and Reject one at a time or all at once; the latest decision with its time
// and Undo; and the host's line where it couldn't list or decide them. The room reads and decides them
// (app/src/room/word/changes-pane.tsx); this part shows what it is given and hands the lawyer's choice back. It wears
// the draft review's cards (draft-pane.tsx's ChangesList, word.css).
import { useEffect, useId, useRef, type RefObject } from "react";
import { IconAccept, IconBackToAnswer } from "../icons/icons.tsx";
import { errorWords } from "../lib/words-errors.ts";
import { mf } from "../lib/words-matter-form.ts";
import { useReaderWords, type ReaderKey } from "../lib/words-reader.ts";
import { langOfText } from "../lib/script.ts";
import { Button } from "../ui/button.tsx";
import { Heading } from "./heading.tsx";
import { isLatinOnly } from "./matter.ts";
import { Say } from "./say.tsx";

/** What a tracked change does, as the host lists it; unknown for a kind from a newer Clauseo. */
export type TrackedKind = "insert" | "delete" | "move" | "format" | "unknown";

/** Accept or reject. */
export type TrackedDecision = "accept" | "reject";

/** One tracked change as the panel shows it: its ids (what a decision names), its author as Word recorded it, when, in
 * the language ("" where the file records no time), and its words, or null for a change with none (formatting). */
export type TrackedChange = {
  key: string;
  ids: readonly number[];
  kind: TrackedKind;
  author: string;
  when: string;
  text: string | null;
};

/** Each kind's label over its words. A kind this version doesn't know shows its words with no label: no label here
 * would be true of it. */
const LABEL: Readonly<Record<TrackedKind, ReaderKey | null>> = {
  insert: "Added",
  delete: "Taken out",
  format: "Formatting changed",
  move: "Moved",
  unknown: null,
};

/** Content as Word holds it: lang="en" where it is Latin only, Hindi where it reads as Hindi. */
const langOf = (text: string) => (isLatinOnly(text) ? "en" : langOfText(text));

function ChangeWords({ kind, text }: { kind: TrackedKind; text: string | null }) {
  const w = useReaderWords();
  const label = LABEL[kind] ?? null;
  if (label === null && text === null) return null;
  const words =
    text === null ? null : kind === "insert" ? (
      <ins className="rd-ins" lang={langOf(text)}>
        {text}
      </ins>
    ) : kind === "delete" ? (
      <del className="rd-del" lang={langOf(text)}>
        {text}
      </del>
    ) : (
      <span lang={langOf(text)}>{text}</span>
    );
  return (
    <div className="rd-change-words">
      <p className="rd-change-run" data-kind={kind}>
        {label !== null && <span className="rd-change-kind">{w.r(label)}</span>}
        {words}
      </p>
    </div>
  );
}

/** The tracked changes of one Word file, beside the work in the wall's place, with Back to the wall. `changes` is null
 * until the list is read, and while the host can't read it (its line says why). Every Accept and Reject is held while
 * a decision or an undo is on its way (`sending`). */
export function WordChanges({
  file,
  changes,
  sending,
  decided,
  line,
  backRef,
  onBack,
  onDecide,
  onUndo,
  onRetry,
}: {
  /** The file's name. */
  file: string;
  changes: readonly TrackedChange[] | null;
  sending: boolean;
  /** The latest decision, with its clock time in English ("3:42 pm"), while it can be undone here. */
  decided: { decision: TrackedDecision; time: string } | null;
  /** What the host refused with, by its code, and whether Try again sends the decision again. */
  line: { code: string; retry: boolean } | null;
  backRef?: RefObject<HTMLButtonElement | null>;
  onBack: () => void;
  onDecide: (ids: readonly number[], decision: TrackedDecision) => void;
  onUndo: () => void;
  onRetry: () => void;
}) {
  const w = useReaderWords();
  const { lang } = w;
  const headId = useId();
  const head = useRef<HTMLHeadingElement>(null);
  // the panel takes the focus as it opens, and again when a decided row leaves the list with the focus in it
  useEffect(() => {
    const at = document.activeElement;
    if (at === null || at === document.body) head.current?.focus();
  }, [changes, decided]);
  const all = changes?.flatMap((c) => c.ids) ?? [];
  return (
    <section className="rd-review wc-pane" aria-labelledby={headId} data-testid="word-changes">
      <div className="wc-bar">
        <Button
          ref={backRef}
          kind="quiet"
          size="small"
          className="pr-s3 pl-s2"
          data-testid="word-changes-back"
          onClick={onBack}
        >
          <IconBackToAnswer size={16} />
          {mf(lang, "Back")}
        </Button>
      </div>
      <header className="wc-head">
        <Heading level={2} className="wall-title" id={headId} ref={head} tabIndex={-1}>
          {w.r("Tracked changes")}
        </Heading>
        <p className="wc-file" lang={langOf(file)}>
          {file}
        </p>
      </header>
      {line !== null && (
        <div className="wc-line" role="alert" data-testid="word-changes-line">
          <p>
            <Say>{errorWords(lang, line.code)}</Say>
          </p>
          {line.retry && (
            <Button
              kind="quiet"
              size="small"
              disabled={sending}
              focusableWhenDisabled
              data-testid="word-changes-retry"
              onClick={onRetry}
            >
              {w.r("Try again")}
            </Button>
          )}
        </div>
      )}
      {decided !== null && (
        <div className="wc-decided" data-testid="word-changes-decided">
          <span className="rd-change-state" role="status">
            {decided.decision === "accept" && <IconAccept size={14} />}
            {decided.decision === "accept"
              ? w.r("Accepted {time}", { time: w.clock(decided.time) })
              : w.r("Rejected {time}", { time: w.clock(decided.time) })}
          </span>
          <Button
            kind="quiet"
            size="small"
            className="rd-undo"
            disabled={sending}
            focusableWhenDisabled
            data-testid="word-changes-undo"
            onClick={onUndo}
          >
            {w.t("Undo")}
          </Button>
        </div>
      )}
      {changes !== null && changes.length === 0 && (
        <p className="rd-review-sub" data-testid="word-changes-empty">
          <Say>{w.r("No tracked changes in this file.")}</Say>
        </p>
      )}
      {changes !== null && changes.length > 0 && (
        <div>
          <div className="rd-review-head">
            <p className="rd-review-sub">
              <Say>
                {w.r("{n} changes, each with its author and time, as Word shows them.", {
                  n: changes.length,
                })}
              </Say>
            </p>
            <div className="rd-review-all">
              <Button
                kind="tone"
                size="small"
                className="rd-accept-all"
                disabled={sending}
                focusableWhenDisabled
                onClick={() => onDecide(all, "accept")}
              >
                <IconAccept size={14} />
                {w.r("Accept all")}
              </Button>
              <Button
                kind="quiet"
                size="small"
                className="rd-reject-all"
                disabled={sending}
                focusableWhenDisabled
                onClick={() => onDecide(all, "reject")}
              >
                {w.r("Reject all")}
              </Button>
            </div>
          </div>
          <ol className="rd-changes">
            {changes.map((c) => {
              const about = w.r("Change by {who}, {when}", { who: c.author, when: c.when });
              return (
                <li key={c.key} className="rd-change" data-state="pending" data-kind={c.kind}>
                  <p className="rd-change-head">
                    <b lang={langOf(c.author)}>{c.author}</b>
                    {c.when !== "" && <span className="rd-change-when">{c.when}</span>}
                  </p>
                  <ChangeWords kind={c.kind} text={c.text} />
                  <div className="rd-change-foot">
                    <span className="rd-change-state">{w.r("Not yet accepted")}</span>
                    <span className="rd-change-acts">
                      <Button
                        kind="tone"
                        size="small"
                        className="rd-accept"
                        aria-label={`${w.r("Accept")}. ${about}`}
                        disabled={sending}
                        focusableWhenDisabled
                        onClick={() => onDecide(c.ids, "accept")}
                      >
                        {w.r("Accept")}
                      </Button>
                      <Button
                        kind="quiet"
                        size="small"
                        className="rd-reject"
                        aria-label={`${w.r("Reject")}. ${about}`}
                        disabled={sending}
                        focusableWhenDisabled
                        onClick={() => onDecide(c.ids, "reject")}
                      >
                        {w.r("Reject")}
                      </Button>
                    </span>
                  </div>
                </li>
              );
            })}
          </ol>
          <p className="rd-review-foot">
            <Say>{w.r("Accepting here accepts the change in the Word file too.")}</Say>
          </p>
        </div>
      )}
    </section>
  );
}
