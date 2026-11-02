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
import { defaultOutcome, mountRoom, summary, type Harness, type Script } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const QUESTION = "Summarise the notice to the bank.";
const field = () => h!.one("composer") as HTMLTextAreaElement;
const H10: Script["next"] = { parts: ["restart"], query: () => ({ threads: [] }) };
const status = (s: "starting" | "running" | "idle" | "agent_exited", agent?: string): ViewEntry => {
  const head = header();
  head.header!.status = s;
  if (agent !== undefined) head.header!.agent = agent;
  return head;
};

/** The room with an empty thread, the question asked and under way with its first chunk; its header names `agent`. */
async function asked(script: Script = {}, agent?: string): Promise<Harness> {
  const room = await mountRoom(script);
  room.deliver([reset([])]);
  await room.type(QUESTION);
  await room.key({ key: "Enter", keyCode: 13 });
  room.deliver([
    upsert(userMessage(5, 1, QUESTION)),
    upsert(agentText(6, 1, "Clause nine requires notice")),
    upsert(status("running", agent)),
  ]);
  return room;
}

const failed = (failure: ErrorCode, ts = Date.now(), agent?: string) => [
  upsert(step(9, { stop: "failed", failure, ts })),
  upsert(status(failure === "agent.exited" ? "agent_exited" : "idle", agent)),
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

test("a relaunch that fails before its turn starts: the second question stays, its failed step card shows, the thread settles to Idle", async () => {
  let sends = 0;
  h = await asked({
    dispatch: (command) => {
      if (command.send_prompt === undefined) return defaultOutcome(command);
      sends += 1;
      return { prompt_accepted: { turn: uuid(sends, 4), message: uuid(sends, 5) } };
    },
  });
  h.deliver(failed("agent.exited"));
  await h.settle();
  // asking again: the host takes the question as a new turn, and the relaunch fails before that turn starts (0002)
  await h.key({ key: "Enter", keyCode: 13 });
  const second = uuid(2, 4);
  const again = userMessage(10, 2, QUESTION);
  again.user_message!.turn = second;
  h.deliver([upsert(again), upsert(status("starting"))]);
  await h.settle();
  h.deliver([
    upsert(step(11, { turn: second, stop: "failed", failure: "agent.exited", ts: Date.now() })),
    upsert(status("idle")),
  ]);
  await h.settle();
  expect(h.all("message-user").map((m) => m.textContent)).toEqual([
    expect.stringContaining(QUESTION),
    expect.stringContaining(QUESTION),
  ]);
  expect(h.all("message-agent").map((m) => m.textContent)).toEqual(["Clause nine requires notice"]);
  const steps = h.all("turn-stopped");
  expect(steps.map((s) => [s.dataset["stopReason"], s.dataset["failure"]])).toEqual([
    ["failed", "agent.exited"],
    ["failed", "agent.exited"],
  ]);
  // each stop shows its mark where it stopped
  expect(steps.map((s) => s.querySelector('[data-testid="stopped-here"]') !== null)).toEqual([
    true,
    true,
  ]);
  // settled: no turn runs, so Ask is back, and the new break's line and question come back
  expect(h.one("cancel")).toBeNull();
  expect(h.one("send")).not.toBeNull();
  expect(h.one("working")).toBeNull();
  expect(h.one("agent-stopped")?.dataset["code"]).toBe("agent.exited");
  expect(field().value).toBe(QUESTION);
});

test("the question comes back without moving the lawyer's focus from a field elsewhere", async () => {
  h = await asked();
  const elsewhere = document.createElement("input");
  document.body.append(elsewhere);
  elsewhere.focus();
  h.deliver(failed("agent.exited"));
  await h.settle();
  expect(field().value).toBe(QUESTION);
  expect(document.activeElement).toBe(elsewhere);
  elsewhere.remove();
});

test("the break is said once: one status region in the composer, the same node from Answering. to the stopped line", async () => {
  h = await asked();
  const regions = () => [...h!.host.querySelectorAll('.room-entry [role="status"]')];
  expect(regions()).toHaveLength(1);
  const before = regions()[0];
  expect(before?.textContent).toContain("Answering.");
  h.deliver(failed("agent.exited"));
  await h.settle();
  expect(regions()).toHaveLength(1);
  expect(regions()[0]).toBe(before);
  expect(before?.textContent).toContain("The answer stopped at");
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
    // Restart asks the question itself, so the field stays empty: no second ask waits there
    expect(field().value).toBe("");
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

test("with H10 faked, an interrupted turn already in the view when the thread opens offers Restart", async () => {
  h = await mountRoom({ next: H10 });
  h.deliver([
    reset(
      [
        userMessage(5, 1, QUESTION),
        agentText(6, 1, "Clause nine"),
        step(9, { stop: "failed", failure: "thread.interrupted" }),
      ],
      status("idle"),
    ),
  ]);
  await h.settle();
  expect(h.one("agent-stopped")?.dataset["code"]).toBe("thread.interrupted");
  expect(h.one("restart")).not.toBeNull();
  expect(field().value).toBe("");
});

test("a break this window didn't ask for, with the header's flip a batch before the step, is said in the one region", async () => {
  // opened mid-answer: no question sent from here, so Answering. follows the header alone
  h = await mountRoom();
  h.deliver([
    reset([userMessage(5, 1, QUESTION), agentText(6, 1, "Clause nine")], status("running")),
  ]);
  await h.settle();
  const regions = () => [...h!.host.querySelectorAll('.room-entry [role="status"]')];
  expect(regions()).toHaveLength(1);
  const region = regions()[0];
  expect(region?.textContent).toContain("Answering.");
  h.deliver([upsert(status("agent_exited"))]);
  await h.settle();
  // nothing stands for a moment: the region stays, empty
  expect(regions()).toEqual([region]);
  expect(region?.textContent).toBe("");
  h.deliver([upsert(step(9, { stop: "failed", failure: "agent.exited", ts: Date.now() }))]);
  await h.settle();
  expect(h.one("agent-stopped")).not.toBeNull();
  expect(regions()).toEqual([region]);
  expect(region?.textContent).toContain("The answer stopped at");
});

test("in Hindi the region tags English names, as the line's words do", async () => {
  h = await asked({ next: H10, display: { language: "hi" } });
  h.deliver(failed("agent.launch_failed"));
  await h.settle();
  const region = h.host.querySelector('.room-entry [role="status"]');
  expect(region?.textContent).toContain(errorWords("hi", "agent.launch_failed"));
  expect(region?.querySelector('[lang="en"]')?.textContent).toBe("Claude");
});

// agent.launch_failed names the open conversation's plan (words-errors.ts's plannedKey): its kind read off the
// thread's agent, Codex's as ChatGPT, and an agent of no kind (a settings file's own id) as Clauseo's conversation
test.each([
  ["codex-acp", "en", "ChatGPT didn't start. Your question is kept."],
  ["codex-acp", "hi", "ChatGPT शुरू नहीं हुआ। आपका प्रश्न रखा गया है।"],
  ["claude-acp", "en", "Claude didn't start. Your question is kept."],
  ["claude-acp", "hi", "Claude शुरू नहीं हुआ। आपका प्रश्न रखा गया है।"],
  ["my-agent", "en", "Clauseo couldn't start this conversation. Your question is kept."],
  ["my-agent", "hi", "Clauseo यह बातचीत शुरू नहीं कर सका। आपका प्रश्न रखा गया है।"],
] as const)(
  "a thread on %s that didn't start says so for its plan, in %s",
  async (agent, language, said) => {
    h = await asked(
      { threads: [{ ...summary(), agent }], next: H10, display: { language } },
      agent,
    );
    h.deliver(failed("agent.launch_failed", Date.now(), agent));
    await h.settle();
    // the working line, with Restart, and the step's row in the thread
    expect(h.one("agent-stopped")?.textContent).toContain(said);
    expect(h.one("turn-stopped")?.textContent).toContain(said);
  },
);

// the other agent.* codes that name Claude take the plan by the same path: the step's row in a Codex thread
test.each([
  ["agent.auth_required", "Sign in to ChatGPT in Codex first. Your question is kept."],
  ["agent.protocol_error", "ChatGPT sent something Clauseo can't read."],
] as const)("a Codex thread's %s names ChatGPT on the step's row", async (code, said) => {
  const agent = "codex-acp";
  h = await asked({ threads: [{ ...summary(), agent }], next: H10 }, agent);
  h.deliver(failed(code, Date.now(), agent));
  await h.settle();
  expect(h.one("turn-stopped")?.textContent).toContain(said);
});
