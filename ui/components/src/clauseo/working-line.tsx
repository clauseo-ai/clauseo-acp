// The working line. While Clauseo works, a row opens upward from the composer as the top of the
// field's own well, divided from the field by tone: the wait icon, what it's waiting for, and the clock time the
// answer is expected, on a raised pill. Where the field has room, the pill stands beside Stop, so the words keep
// one line. Its words and times change in place; no clock runs: it changes only when the host says something, and
// nothing loops, shimmers or spins but the pen, which writes in the icon's place while Clauseo writes (pen-mark.tsx;
// Rohan's call over the design's no-indicator rule, 2026-11). Errors stand on the same well, in concrete and never in
// red, because red means the other side, and led by the break in the clock's place, so they look broken before
// they're read (the icon pass, §12). The field below keeps what the lawyer wrote.
import { lazy, Suspense, type ReactNode } from "react";
import { late } from "../lib/late.ts";
import { Button } from "../ui/button.tsx";
import { IconBreak, IconChecking, IconFold, IconQuestion } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { sentence } from "./matter.ts";
import { PenMark } from "./pen-mark.tsx";
import { Say } from "./say.tsx";

/** What the working line says. One check names what it reads; a series counts the judgments and names the one in
 * hand; queued waits for another answer in the matter; limit, retrying and stopped are the errors.
 *
 * `working`, optional on a check, a series and a wait: Clauseo is writing now. The pen writes in the wait icon's
 * place until it is false or the row goes, so the lawyer sees the answer is still coming. What is being done now is
 * the state's own words (a wait's `words`, such as "Answering." or "Reading the complaint."), so assistive technology
 * hears it too. Left out, the row shows the still wait icon, as before. */
export type WorkingState =
  /** name and waitingFor are the app's words, in the language: "Acme Ltd v. State", "judgments since 2001 that cite
   * it". expected, on a check and a series, is the clock time the answer is expected, or null when the app has none:
   * then the row has no pill. */
  | { kind: "check"; name: string; waitingFor: string; expected: string | null; working?: boolean }
  | {
      kind: "series";
      total: number;
      current: string | null;
      expected: string | null;
      working?: boolean;
    }
  | { kind: "queued"; expected: string }
  | { kind: "limit"; resets: string }
  | { kind: "retrying"; at: string; lastReply: string; onRetry?: () => void }
  | { kind: "stopped"; onRestart?: () => void }
  /** Any other wait, in words the caller has put in the language: what it waits for, and its clock time, if it has
   * one, such as Claude getting ready on first launch, or the matter's first read. Actions, optional, stand at the
   * row's right, as an error's do: a download's Try again. */
  | {
      kind: "wait";
      words: string;
      pill: string | null;
      /** Optional: Clauseo is writing now, so the pen writes in the icon's place. */
      working?: boolean;
      /** Optional: the wait is on the lawyer, a permission prompt or a question: the question mark leads, never the
       * pen. */
      asks?: boolean;
      /** Optional: the wait's own ways on, such as a download's Try again, Restart or Sign in again. */
      actions?: ReactNode;
      /** Optional: the details those ways fold open. */
      details?: string | null;
      /** Optional: the error's code (an ErrorCode), when the wait is an error, set as the row's data-code. */
      code?: string;
    };

/** What leads the row: the question mark while it waits on the lawyer; the pen while Clauseo writes; the break on an error (a stop, not responding, or a wait that
 * carries an error's code), so a broken line looks broken before it's read; the clock on every other wait. */
function leadOf(state: WorkingState): "pen" | "break" | "question" | "wait" {
  if (state.kind === "wait" && state.asks === true) return "question";
  if ("working" in state && state.working === true) return "pen";
  if (state.kind === "stopped" || state.kind === "retrying") return "break";
  return state.kind === "wait" && state.code !== undefined ? "break" : "wait";
}

/** The row's lead, by leadOf: an icon at its own 16 px, so the words start over the field's own as the prototype has
 * them, or the pen in its fixed box (20 px by 110/36 of it, scaled with the text size, never with the language's line),
 * so nothing moves while it writes. */
function Lead({ state }: { state: WorkingState }) {
  const lead = leadOf(state);
  return (
    <span className="working-lead" data-lead={lead}>
      {lead === "pen" ? (
        <PenMark className="working-icon working-pen" />
      ) : lead === "break" ? (
        <IconBreak size={16} className="working-icon" />
      ) : lead === "question" ? (
        <IconQuestion size={16} className="working-icon" />
      ) : (
        <IconChecking size={16} className="working-icon" />
      )}
    </span>
  );
}

type Parts = { words: string; pill: string | null; actions: ReactNode; details: string | null };

/** What the line says for a state: its words, the time on its pill, and an error's actions and details. */
function useParts(state: WorkingState): Parts {
  const w = useThreadWords();
  const s = (x: string) => sentence(w.lang, x);
  const expectedBy = (at: string | null) =>
    at === null ? null : w.t("Expected by {time}", { time: w.clock(at) });
  switch (state.kind) {
    case "check":
      return {
        words: w.th("Checking {name}. Reading {waitingFor}.", {
          name: state.name,
          waitingFor: state.waitingFor,
        }),
        pill: expectedBy(state.expected),
        actions: null,
        details: null,
      };
    case "series": {
      const series = w.th("Checking {n} judgments against later decisions", { n: state.total });
      return {
        words:
          state.current === null
            ? s(series)
            : `${s(series)} ${w.th("Now {name}.", { name: state.current })}`,
        pill: expectedBy(state.expected),
        actions: null,
        details: null,
      };
    }
    case "queued":
      return {
        words: w.th("Clauseo is still answering in this matter. Your question waits until then."),
        pill: w.t("Expected by {time}", { time: w.clock(state.expected) }),
        actions: null,
        details: null,
      };
    case "limit":
      return {
        words: `${s(w.t("Claude plan limit reached"))} ${w.th("It resets at {time}. Your question is saved.", { time: w.clock(state.resets) })}`,
        pill: w.th("Asking at {time}", { time: w.clock(state.resets) }),
        actions: null,
        details: null,
      };
    case "retrying":
      return {
        words: w.th("Claude is not responding."),
        pill: w.th("Retrying at {time}", { time: w.clock(state.at) }),
        details: w.th(
          "Claude last replied at {time}. Your question and the answer so far are kept.",
          {
            time: w.clock(state.lastReply),
          },
        ),
        actions: (
          <Button kind="quiet" size="small" className="working-act" onClick={state.onRetry}>
            <span className="deva-nudge">{w.th("Retry now")}</span>
          </Button>
        ),
      };
    case "wait":
      return {
        words: state.words,
        pill: state.pill,
        actions: state.actions ?? null,
        details: state.details ?? null,
      };
    case "stopped":
      return {
        words: w.th("Clauseo stopped. Your question and the answer so far are kept."),
        pill: null,
        details: null,
        actions: (
          <Button kind="tone" size="small" className="working-act" onClick={state.onRestart}>
            <span className="deva-nudge">{w.th("Restart")}</span>
          </Button>
        ),
      };
  }
}

/** The expected time, or the time an error clears, on its raised pill: in the row on a narrow field, beside Stop on
 * a field with room. Seen, not read twice: the row says it to assistive technology. */
function TimePill({ children, place }: { children: string; place: "row" | "field" }) {
  return (
    <span className="working-time" data-in={place} aria-hidden="true">
      <Say>{children}</Say>
    </span>
  );
}

/** What assistive technology hears of the line: its words, with English names tagged in Hindi, and its time. The
 * composer holds the status region these go into, mounted whenever the composer is, so each change (Answering., then
 * a break's words, or nothing between) is said once in place; the row's own words are seen, not read twice. */
export function WorkingSaid({ state }: { state: WorkingState }) {
  const { words, pill } = useParts(state);
  return (
    <>
      <Say>{words}</Say>
      {pill !== null && ` ${pill}`}
    </>
  );
}

/** The pill's place beside Stop, in the field's row. Shown only where the field has room for it. */
export function WorkingTime({ state }: { state: WorkingState }) {
  const { pill } = useParts(state);
  return pill === null ? null : <TimePill place="field">{pill}</TimePill>;
}

/** The details an error folds open, with the row's Details trigger: its own chunk, since most rows have none. */
const WorkingDetails = lazy(() =>
  late(import("./working-details.tsx").then((m) => ({ default: m.WorkingDetails }))),
);

/** The row itself, the well's top. */
export function WorkingLine({
  state,
  testId,
}: {
  state: WorkingState;
  testId?: string | undefined;
}) {
  const wd = useThreadWords();
  const { words, pill, actions, details } = useParts(state);
  const row = (more: ReactNode) => (
    <div
      className="working"
      data-kind={state.kind}
      data-code={state.kind === "wait" ? state.code : undefined}
      data-lead={leadOf(state)}
      data-testid={testId}
    >
      <Lead state={state} />
      <span className="working-text" aria-hidden="true">
        <span>
          <Say>{words}</Say>
        </span>
      </span>
      {pill !== null && <TimePill place="row">{pill}</TimePill>}
      {actions !== null && (
        <span className="working-acts">
          {actions}
          {more}
        </span>
      )}
    </div>
  );
  if (details === null) return row(null);
  // until the fold is in, its trigger stands as a plain button of the same look
  return (
    <>
      <Suspense
        fallback={row(
          <Button kind="quiet" size="small" className="working-more">
            <span className="deva-nudge">{wd.th("Details")}</span>
            <IconFold size={12} className="work-chev" />
          </Button>,
        )}
      >
        <WorkingDetails row={row} details={details} />
      </Suspense>
    </>
  );
}
