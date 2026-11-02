// @vitest-environment happy-dom
// U10 through the room, as a break arrives while the lawyer watches: the stopped line with its time and no Restart, the
// answer so far kept, the question back in an empty field (and only an empty one), asking again; a reopened thread's
// break with no line; with H10 faked, Restart for the codes it mends, hidden during a turn, dispatching restart_agent.
import { brk } from "@clauseo/components/lib/words-breaks.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { afterEach, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import type { ErrorCode, ViewEntry } from "../transport/protocol.ts";
import { stamp } from "./thread/clock.ts";
import { defaultOutcome, mountRoom, type Harness, type Script } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const QUESTION = "Summarise the notice to the bank.";
const field = () => h!.one("composer") as HTMLTextAreaElement;
const H10: Script["next"] = { parts: ["restart"], query: () => ({ threads: [] }) };
const status = (s: "running" | "idle" | "agent_exited"): ViewEntry => {
  const head = header();
  head.header!.status = s;
  return head;
};

/** The room with an empty thread, the question asked and under way with its first chunk. */
async function asked(script: Script = {}): Promise<Harness> {
  const room = await mountRoom(script);
  room.deliver([reset([])]);
  await room.type(QUESTION);
  await room.key({ key: "Enter", keyCode: 13 });
  room.deliver([
    upsert(userMessage(5, 1, QUESTION)),
    upsert(agentText(6, 1, "Clause nine requires notice")),
    upsert(status("running")),
  ]);
  return room;
}

const failed = (failure: ErrorCode, ts = Date.now()) => [
  upsert(step(9, { stop: "failed", failure, ts })),
  upsert(status(failure === "agent.exited" ? "agent_exited" : "idle")),
];

test("Claude stopping mid-answer: the stopped line with its time and no Restart, the answer so far kept, the question back in the empty field", async () => {
  // each send_prompt starts its own turn, as the host's do
  let sends = 0;
  h = await asked({
    dispatch: (command) => {
      if (command.send_prompt === undefined) return defaultOutcome(command);
      sends += 1;
      return { prompt_accepted: { turn: uuid(sends, 4), message: uuid(sends, 5) } };
    },
  });
  expect(field().value).toBe("");
  const ts = Date.now();
  h.deliver(failed("agent.exited", ts));
  await h.settle();
  const line = h.one("agent-stopped");
  expect(line?.dataset["code"]).toBe("agent.exited");
  expect(line?.textContent).toContain(
    brk("en", "The answer stopped at {time}. Your question and the answer so far are kept.", {
      time: stamp("en", ts, ts),
    }),
  );
  expect(h.one("restart")).toBeNull();
  expect(h.one("turn-stopped")?.dataset["failure"]).toBe("agent.exited");
  expect(h.one("stopped-here")).not.toBeNull();
  expect(h.one("message-user")?.textContent).toContain(QUESTION);
  expect(h.one("message-agent")?.textContent).toBe("Clause nine requires notice");
  expect(field().value).toBe(QUESTION);
  // asking again sends the question through send_prompt, which relaunches Claude
  await h.key({ key: "Enter", keyCode: 13 });
  expect(h.sent("send_prompt").map((c) => c.content[0]?.text?.text)).toEqual([QUESTION, QUESTION]);
  expect(h.one("agent-stopped")).toBeNull();
  expect(h.one("working")).not.toBeNull();
});

test("words typed before the break keep their place: the question comes back only into an empty field", async () => {
  h = await asked();
  await h.type("And clause ten?");
  h.deliver(failed("agent.exited"));
  await h.settle();
  expect(h.one("agent-stopped")).not.toBeNull();
  expect(field().value).toBe("And clause ten?");
});

test("a reopened thread's break: the step row's mark alone, no stopped line, an empty field", async () => {
  h = await mountRoom();
  h.deliver([
    reset(
      [
        userMessage(5, 1, QUESTION),
        agentText(6, 1, "Clause nine"),
        step(9, { stop: "failed", failure: "agent.exited" }),
      ],
      status("agent_exited"),
    ),
  ]);
  await h.settle();
  expect(h.one("stopped-here")).not.toBeNull();
  expect(h.one("agent-stopped")).toBeNull();
  expect(field().value).toBe("");
});

test("with H10 faked: Restart for agent.exited, agent.launch_failed and thread.interrupted, dispatching restart_agent", async () => {
  for (const code of ["agent.exited", "agent.launch_failed", "thread.interrupted"] as const) {
    h = await asked({ next: H10 });
    // hidden during the turn
    expect(h.one("restart")).toBeNull();
    h.deliver(failed(code));
    await h.settle();
    const line = h.one("agent-stopped");
    expect(line?.dataset["code"]).toBe(code);
    if (code !== "agent.exited") expect(line?.textContent).toContain(errorWords("en", code));
    await h.click(h.one("restart"));
    expect(h.sent("restart_agent")).toEqual([{ thread: THREAD }]);
    // accepted: the line steps aside while the host asks again
    expect(h.one("agent-stopped")).toBeNull();
    await h.unmount();
    h = null;
  }
});

test("with H10 faked: no Restart for a code a restart can't mend, and a refused Restart says its code", async () => {
  h = await asked({ next: H10 });
  h.deliver(failed("agent.protocol_error"));
  await h.settle();
  expect(h.one("restart")).toBeNull();
  expect(h.one("agent-stopped")).toBeNull();
  expect(h.one("turn-stopped")?.textContent).toBe(errorWords("en", "agent.protocol_error"));
  await h.unmount();

  h = await asked({
    next: H10,
    dispatch: (command) => {
      if (command.restart_agent !== undefined)
        throw { code: "thread.turn_running", message: "SECRET", retryable: true };
      return defaultOutcome(command);
    },
  });
  h.deliver(failed("agent.exited"));
  await h.settle();
  await h.click(h.one("restart"));
  const line = h.one("agent-stopped");
  expect(line?.dataset["code"]).toBe("thread.turn_running");
  expect(line?.textContent).toContain(errorWords("en", "thread.turn_running"));
  expect(h.host.textContent).not.toContain("SECRET");
});

test("Restart is hidden while a turn runs again", async () => {
  h = await asked({ next: H10 });
  h.deliver(failed("agent.exited"));
  await h.settle();
  expect(h.one("restart")).not.toBeNull();
  h.deliver([upsert(status("running"))]);
  await h.settle();
  expect(h.one("restart")).toBeNull();
});
