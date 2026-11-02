// @vitest-environment happy-dom
// thread-break.tsx's parts, without the room: which turn is a break, the stopped line (with Restart only once the host
// has it, H10 faked), and the stream-failed line with Try again only for a retryable code. room.test.tsx's break tests
// drive the same lines through the room.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { WorkingLine } from "@clauseo/components/clauseo/working-line.tsx";
import { brk as say, streamFailedWords } from "@clauseo/components/lib/words-breaks.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test, vi } from "vitest";
import { agentText, step, userMessage } from "../testing/stream-script.ts";
import type { EntryId, ViewEntry } from "../transport/protocol.ts";
import { entryIdOf, type StreamView } from "../store/view.ts";
import { stoppedLine, streamFailedLine } from "./break-lines.ts";
import { pickBreak, refillOf, RESTARTS, type Break } from "./thread-break.ts";
import { clockTime } from "./thread/clock.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => document.body.replaceChildren());

/** A view of `entries`, in order, as the store holds one. */
function viewOf(entries: ViewEntry[]): StreamView {
  const ids: EntryId[] = entries.map(entryIdOf);
  return {
    ids,
    entries: new Map(entries.map((e, i) => [ids[i]!, e])),
    texts: new Map(),
  } as unknown as StreamView;
}

const ASKED = userMessage(5, 1, "Summarise the notice.");
const SO_FAR = agentText(6, 1, "The notice says");
const exited = (order = 7) => step(order, { stop: "failed", failure: "agent.exited" });

function draw(line: ReturnType<typeof stoppedLine>) {
  const host = document.createElement("div");
  document.body.append(host);
  act(() =>
    createRoot(host).render(
      line === null ? null : <WorkingLine state={line.state} testId={line.testId} />,
    ),
  );
  return host;
}

test("a turn that ended failed, with nothing asked since, is a break that carries its question; anything else is not", () => {
  const view = viewOf([ASKED, SO_FAR, exited()]);
  const found = pickBreak(view);
  expect(found).toMatchObject({ code: "agent.exited", question: "Summarise the notice." });
  // the same object while nothing changes: the working line renders nothing on a flush
  expect(pickBreak(view)).toBe(found);
  expect(pickBreak(viewOf([ASKED, SO_FAR, step(7)]))).toBeNull();
  expect(pickBreak(viewOf([ASKED, exited(), userMessage(8, 2, "Again")]))).toBeNull();
  expect(pickBreak(viewOf([ASKED, step(7, { stop: "failed" })]))?.code).toBe("unknown");
  expect(pickBreak(undefined)).toBeUndefined();
});

const brk = (code: string): Break => ({
  id: "step:x",
  step: {
    order: 7,
    turn: "t",
    stop: "failed",
    failure: code as never,
    files: [],
    skipped: [],
    undone: false,
    ts: Date.UTC(2026, 10, 2, 11, 1),
  },
  code,
  question: "Q",
  seen: Date.UTC(2026, 10, 2, 11, 1),
});

test("agent.exited before H10: the stopped line with its time, no Restart, no details", () => {
  const b = brk("agent.exited");
  const host = draw(stoppedLine("en", b, b.step.ts, null));
  const row = host.querySelector<HTMLElement>('[data-testid="agent-stopped"]');
  expect(row?.dataset["code"]).toBe("agent.exited");
  expect(row?.textContent).toContain(
    say("en", "The answer stopped at {time}. Your question and the answer so far are kept.", {
      time: clockTime("en", b.step.ts),
    }),
  );
  expect(host.querySelector('[data-testid="restart"]')).toBeNull();
  expect(host.textContent).not.toContain("Details");
});

test("with H10 faked: Restart for agent.exited, agent.launch_failed and thread.interrupted only, and it calls restart", () => {
  expect([...RESTARTS].sort()).toEqual([
    "agent.exited",
    "agent.launch_failed",
    "thread.interrupted",
  ]);
  for (const code of RESTARTS) {
    const onRestart = vi.fn();
    const host = draw(stoppedLine("en", brk(code), 0, { onRestart, refused: null }));
    const restart = host.querySelector<HTMLElement>('[data-testid="restart"]');
    expect(restart).not.toBeNull();
    act(() => restart!.click());
    expect(onRestart).toHaveBeenCalledOnce();
    if (code !== "agent.exited") expect(host.textContent).toContain(errorWords("en", code));
  }
  // with Restart, agent.exited folds open what Restart does
  const exitedHost = draw(
    stoppedLine("en", brk("agent.exited"), 0, { onRestart: () => {}, refused: null }),
  );
  expect(exitedHost.textContent).toContain("Details");
  // a restart can't mend these, so no line: the step row says them
  for (const code of ["agent.protocol_error", "agent.unsupported_version"])
    expect(stoppedLine("en", brk(code), 0, { onRestart: () => {}, refused: null })).toBeNull();
  // and before H10 only agent.exited takes the line
  expect(stoppedLine("en", brk("agent.launch_failed"), 0, null)).toBeNull();
  expect(stoppedLine("en", brk("thread.interrupted"), 0, null)).toBeNull();
});

test("a refused Restart shows its own code's words, never its message; Restart again only if the refusal is retryable; no details", () => {
  const retry = draw(
    stoppedLine("en", brk("agent.exited"), 0, {
      onRestart: () => {},
      refused: { code: "thread.turn_running", message: "SECRET", retryable: true },
    }),
  );
  const row = retry.querySelector<HTMLElement>('[data-testid="agent-stopped"]');
  expect(row?.dataset["code"]).toBe("thread.turn_running");
  expect(row?.textContent).toContain(errorWords("en", "thread.turn_running"));
  expect(retry.textContent).not.toContain("SECRET");
  expect(retry.querySelector('[data-testid="restart"]')).not.toBeNull();
  expect(retry.textContent).not.toContain("Details");
  const final = draw(
    stoppedLine("en", brk("agent.exited"), 0, {
      onRestart: () => {},
      refused: { code: "command.unknown_thread", message: "SECRET", retryable: false },
    }),
  );
  expect(final.querySelector('[data-testid="agent-stopped"]')?.textContent).toContain(
    errorWords("en", "command.unknown_thread"),
  );
  expect(final.querySelector('[data-testid="restart"]')).toBeNull();
});

test("the stopped question comes back only before H10, only for the codes asking again mends, only for a live break", () => {
  const restart = { restartAgent: async () => {} };
  for (const code of RESTARTS)
    expect(refillOf(brk(code), true, null)).toEqual({ key: "step:x", text: "Q" });
  expect(refillOf(brk("agent.exited"), true, restart)).toBeNull();
  expect(refillOf(brk("agent.exited"), false, null)).toBeNull();
  expect(refillOf(brk("agent.protocol_error"), true, null)).toBeNull();
  expect(refillOf(brk("agent.unsupported_version"), true, null)).toBeNull();
});

test("a stream that failed: its line and code; Try again only for a retryable code, and it calls retry", () => {
  for (const retryable of [true, false]) {
    const onRetry = vi.fn();
    const error = { code: "store.io" as const, message: "SECRET-for-logs", retryable };
    const line = streamFailedLine("hi", error, onRetry);
    const host = draw(line);
    const row = host.querySelector<HTMLElement>('[data-testid="stream-failed"]');
    expect(row?.dataset["code"]).toBe("store.io");
    expect(row?.textContent).toContain(streamFailedWords("hi", "store.io", retryable));
    expect(host.textContent).not.toContain("SECRET");
    const retry = host.querySelector<HTMLElement>('[data-testid="retry"]');
    expect(retry !== null).toBe(retryable);
    if (retry !== null) {
      act(() => retry.click());
      expect(onRetry).toHaveBeenCalledOnce();
    }
  }
});
