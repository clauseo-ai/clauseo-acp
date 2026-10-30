// The working line. While Clauseo works, a row opens upward from the composer as the top of the
// field's own well, divided from the field by tone: the wait icon, what it's waiting for, and the clock time the
// answer is expected, on a raised pill. Where the field has room, the pill stands beside Stop, so the words keep
// one line. Its words and times change in place; nothing loops, shimmers or spins, and no clock runs: it changes
// only when the host says something. Errors stand on the same well, in concrete and never in red, because red means
// the other side. The field below keeps what the lawyer wrote.
import type { ReactNode } from "react";
import { Collapsible as BaseCollapsible } from "@base-ui/react/collapsible";
import { Button } from "../ui/button.tsx";
import { CollapsiblePanel } from "../ui/collapsible.tsx";
import { IconChecking, IconFold } from "../icons/icons.tsx";
import { useThreadWords } from "../lib/words-thread.ts";
import { sentence } from "./matter.ts";
import { Say } from "./say.tsx";

/** What the working line says. One check names what it reads; a series counts the judgments and names the one in
 * hand; queued waits for another answer in the matter; limit, retrying and stopped are the errors. */
export type WorkingState =
  /** name and waitingFor are the app's words, in the language: "Acme Ltd v. State", "judgments since 2001 that cite
   * it". */
  | { kind: "check"; name: string; waitingFor: string; expected: string }
  | { kind: "series"; total: number; current: string | null; expected: string }
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
      /** Optional: the wait's own ways on, such as a download's Try again, Restart or Sign in again. */
      actions?: ReactNode;
      /** Optional: the details those ways fold open. */
      details?: string | null;
    };

type Parts = { words: string; pill: string | null; actions: ReactNode; details: string | null };

/** What the line says for a state: its words, the time on its pill, and an error's actions and details. */
function useParts(state: WorkingState): Parts {
  const w = useThreadWords();
  const s = (x: string) => sentence(w.lang, x);
  switch (state.kind) {
    case "check":
      return {
        words: w.th("Checking {name}. Reading {waitingFor}.", {
          name: state.name,
          waitingFor: state.waitingFor,
        }),
        pill: w.t("Expected by {time}", { time: w.clock(state.expected) }),
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
        pill: w.t("Expected by {time}", { time: w.clock(state.expected) }),
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

/** The pill's place beside Stop, in the field's row. Shown only where the field has room for it. */
export function WorkingTime({ state }: { state: WorkingState }) {
  const { pill } = useParts(state);
  return pill === null ? null : <TimePill place="field">{pill}</TimePill>;
}

/** The row itself, the well's top. */
export function WorkingLine({ state }: { state: WorkingState }) {
  const w = useThreadWords();
  const { words, pill, actions, details } = useParts(state);
  const row = (
    <div className="working" data-kind={state.kind} role="status">
      <IconChecking size={16} className="working-icon" />
      <span className="working-text">
        <span>
          <Say>{words}</Say>
          {pill !== null && <span className="sr-only"> {pill}</span>}
        </span>
      </span>
      {pill !== null && <TimePill place="row">{pill}</TimePill>}
      {actions !== null && (
        <span className="working-acts">
          {actions}
          {details !== null && (
            <BaseCollapsible.Trigger
              className="working-more"
              render={<Button kind="quiet" size="small" />}
            >
              <span className="deva-nudge">{w.th("Details")}</span>
              <IconFold size={12} className="work-chev" />
            </BaseCollapsible.Trigger>
          )}
        </span>
      )}
    </div>
  );
  if (details === null) return row;
  return (
    <BaseCollapsible.Root className="working-wrap">
      {row}
      <CollapsiblePanel>
        <p className="working-details">
          <Say>{details}</Say>
        </p>
      </CollapsiblePanel>
    </BaseCollapsible.Root>
  );
}
