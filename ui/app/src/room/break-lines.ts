// The working line's states for a break (thread-break.ts), with item 16's words: the stopped answer, with Restart once
// the host has it (H10), and a stream that failed, with Try again only for a retryable code. In the thread's chunk
// (stopped-mark.tsx imports it), so the first paint carries none of it; it hands itself to thread-break.ts as it runs.
import { createElement } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { brk as say, streamFailedWords } from "@clauseo/components/lib/words-breaks.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import type { HostError } from "../transport/protocol.ts";
import { RestartButton, RetryButton } from "./break-buttons.tsx";
import { provideBreakLines, RESTARTS, TIMED, type Break, type BreakLine } from "./thread-break.ts";
import { stamp } from "./thread/clock.ts";

/** The working line for a stopped answer: agent.exited with its time, or, once the host restarts (H10), any code
 * Restart mends, with Restart; with H10 and agent.exited, the details say what Restart does. Null otherwise: the step
 * row's words say the rest. A refused Restart says its refusal's words, offers Restart again only when the refusal
 * says another try may work, and folds open no details. */
export function stoppedLine(
  lang: Lang,
  brk: Break,
  now: number,
  restart: { onRestart: () => void; refused: HostError | null } | null,
): BreakLine | null {
  const mends = restart !== null && RESTARTS.has(brk.code);
  if (brk.code !== TIMED && !mends) return null;
  const refused = restart?.refused ?? null;
  const code = refused?.code ?? brk.code;
  const words =
    refused === null && brk.code === TIMED
      ? say(lang, "The answer stopped at {time}. Your question and the answer so far are kept.", {
          time: stamp(lang, brk.step.ts, now),
        })
      : errorWords(lang, code);
  const offers = mends && (refused === null || refused.retryable);
  return {
    testId: "agent-stopped",
    state: {
      kind: "wait",
      words,
      pill: null,
      code,
      ...(offers
        ? {
            actions: createElement(RestartButton, { lang, onRestart: restart.onRestart }),
            ...(refused === null && brk.code === TIMED
              ? {
                  details: say(
                    lang,
                    "The answer stopped with no reason given. Restart asks your question once more. The answer so far stays above.",
                  ),
                }
              : {}),
          }
        : {}),
    },
  };
}

/** The working line for a stream that failed: that the thread stopped coming in and its code's sentence, with Try
 * again only when the code says another try may work. */
export function streamFailedLine(lang: Lang, error: HostError, onRetry: () => void): BreakLine {
  return {
    testId: "stream-failed",
    state: {
      kind: "wait",
      words: streamFailedWords(lang, error.code, error.retryable),
      pill: null,
      code: error.code,
      ...(error.retryable
        ? {
            actions: createElement(RetryButton, { lang, onRetry }),
          }
        : {}),
    },
  };
}

provideBreakLines({ stoppedLine, streamFailedLine });
