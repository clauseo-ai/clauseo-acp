// @vitest-environment happy-dom
// U10 through the room: every code's words where the host refuses (never HostError.message), a code this version
// doesn't know as internal's line, and each failed step's row: the mark with its time for agent.exited, its code's
// words for the rest, with data-failure.
import { ERROR_WORDS, errorWords, type ErrorCode } from "@clauseo/components/lib/words-errors.ts";
import { brk } from "@clauseo/components/lib/words-breaks.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, reset, step, userMessage, uuid } from "../testing/stream-script.ts";
import { whenOf } from "./thread/clock.ts";
import { mountRoom, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const SECRET = "SECRET-DETAIL for logs";

test("a refused send shows its code's line on the working line for every code, never the message; an unknown code reads as internal", async () => {
  const codes = [...(Object.keys(ERROR_WORDS) as ErrorCode[]), "store.melted"];
  for (const code of codes) {
    h = await mountRoom({
      dispatch: (command) => {
        if (command.send_prompt !== undefined) throw { code, message: SECRET, retryable: false };
        return "done";
      },
    });
    await h.type("Summarise the notice");
    await h.click(h.one("send"));
    const line = h.one("error");
    expect(line?.dataset["code"]).toBe(code);
    // U7: a full queue says the cap, matters/queue-full's line, in place of U3's words
    expect(line?.textContent).toContain(
      code === "thread.queue_full" ? "The queue holds 10." : errorWords("en", code),
    );
    expect(h.host.textContent).not.toContain(SECRET);
    if (code === "store.melted" || code === "unknown")
      expect(line?.textContent).toContain(errorWords("en", "internal"));
    await h.unmount();
    h = null;
  }
  // a room per code: some 30 mounts, which a busy machine can take past the default 5 s
}, 20_000);

test("a failed step: agent.exited marks where it stopped with its time; every other code keeps its words; data-failure on each", async () => {
  h = await mountRoom();
  const failures = [
    "agent.exited",
    "thread.interrupted",
    "agent.launch_failed",
    "agent.protocol_error",
  ] as const;
  h.deliver([
    reset(
      failures.flatMap((failure, i) => [
        userMessage(10 * i + 5, i + 1, `Question ${i}`),
        agentText(10 * i + 6, i + 1, `Answer ${i}`),
        step(10 * i + 7, { stop: "failed", failure, turn: uuid(i + 1, 4) }),
      ]),
    ),
  ]);
  const rows = h.all("turn-stopped");
  expect(rows.map((r) => r.dataset["failure"])).toEqual([...failures]);
  expect(rows.map((r) => r.dataset["stopReason"])).toEqual(failures.map(() => "failed"));
  // 1970 is another day: the mark names it
  expect(rows[0]?.textContent).toBe(
    brk("en", "The answer stopped here on {day} at {time}", {
      day: whenOf("en", 7000, Date.now()).day ?? "",
      time: whenOf("en", 7000, Date.now()).time,
    }),
  );
  for (const [i, failure] of failures.entries()) {
    if (i === 0) continue;
    expect(rows[i]?.textContent).toBe(errorWords("en", failure));
    expect(rows[i]?.querySelector('[data-testid="stopped-here"]')).toBeNull();
  }
  // the answer so far stays in the thread
  expect(h.all("message-agent").map((a) => a.textContent)).toEqual(
    failures.map((_, i) => `Answer ${i}`),
  );
});
