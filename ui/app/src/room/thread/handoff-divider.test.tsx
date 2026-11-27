// @vitest-environment happy-dom
// The handover's divider in the thread (handoff.ts, handoff-divider.tsx; host #1349's "handoff" entry): it sits just
// before the question it went with, never in its own place; it reads pending, given or failed ("Context not
// restored", red, with the failed mark); "What was passed on" opens the counts; a failed one has no button of its
// own: it says the turn's Try again carries it (the host owes the handover to the thread's next turn), only while the
// thread is on the new agent and nothing runs, and that Try again sends the question; and a reload draws it again.
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { HandoffEntryStatus, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { canTryAgain, endText, handoffFor } from "./handoff.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** A question of its own turn, its answer and its end. */
function exchange(k: number, text: string, answered = true): ViewEntry[] {
  const turn = uuid(k, 4);
  const q = userMessage(k * 10, k, text);
  const a = agentText(k * 10 + 1, k, `Answer ${k}.`);
  return [
    { user_message: { ...q.user_message!, turn } },
    ...(answered ? [{ agent_text: { ...a.agent_text!, turn } }] : []),
    step(k * 10 + 9, { turn, ...(answered ? {} : { stop: "failed" as const }) }),
  ];
}

const handoff = (status: HandoffEntryStatus, leftOut = 2): ViewEntry => ({
  handoff: {
    order: 35,
    handoff: uuid(1, 12),
    message: uuid(3, 5),
    from: { agent: "claude-acp", model: { id: "opus", name: "Opus 5.5" } },
    to: { agent: "codex-acp" },
    answers_covered: 2,
    items_included: 7,
    items_left_out: leftOut,
    status,
    ts: 35_000,
  },
});

/** The thread on ChatGPT (the handoff's new agent) unless `agent` says otherwise. */
function headOn(agent = "codex-acp"): ViewEntry {
  const head = header();
  head.header!.agent = agent;
  return head;
}

function view(status: HandoffEntryStatus, leftOut?: number): ViewEntry[] {
  return [
    ...exchange(1, "First question"),
    ...exchange(2, "Second question"),
    ...exchange(3, "Third question", status !== "failed"),
    handoff(status, leftOut),
  ];
}

async function shown(status: HandoffEntryStatus, agent?: string, leftOut?: number) {
  h = await mountRoom();
  h.deliver([reset(view(status, leftOut), headOn(agent))]);
  await h.settle();
  return h;
}

test("given: the quiet divider names both ends, only what is known, just before its question", async () => {
  const room = await shown("delivered");
  const dividers = room.all("handoff-divider");
  expect(dividers).toHaveLength(1);
  const d = dividers[0]!;
  expect(d.dataset["state"]).toBe("delivered");
  expect(room.one("handoff-line")?.textContent).toBe("Context handoff · Claude Opus 5.5 → ChatGPT");
  expect(d.querySelector("svg")).toBeNull();
  // the very next element is the question it went with, the third
  const questions = room.all("message-user");
  expect(questions).toHaveLength(3);
  expect(d.nextElementSibling).toBe(questions[2]!);
  expect(room.one("handoff-again")).toBeNull();
});

test("pending reads Context handoff pending", async () => {
  const room = await shown("pending");
  expect(room.one("handoff-divider")?.dataset["state"]).toBe("pending");
  expect(room.one("handoff-line")?.textContent).toBe(
    "Context handoff pending · Claude Opus 5.5 → ChatGPT",
  );
});

test("failed reads Context not restored, red with the failed mark", async () => {
  const room = await shown("failed");
  const d = room.one("handoff-divider")!;
  expect(room.one("handoff-line")?.textContent).toBe(
    "Context not restored · Claude Opus 5.5 → ChatGPT",
  );
  expect(d.querySelector(".text-red-text svg")).not.toBeNull();
});

test("What was passed on opens the counts; none left out says nothing of it", async () => {
  const room = await shown("delivered");
  expect(room.one("handoff-passed-on")).toBeNull();
  await room.click(room.one("handoff-more"));
  expect(room.one("handoff-passed-on")?.textContent).toBe(
    "Earlier answers: 2 · Parts of the conversation passed on: 7 · Left out for lack of room: 2",
  );
  await room.unmount();
  h = null;
  const none = await shown("delivered", undefined, 0);
  await none.click(none.one("handoff-more"));
  expect(none.one("handoff-passed-on")?.textContent).toBe(
    "Earlier answers: 2 · Parts of the conversation passed on: 7",
  );
});

test("a failed handoff has no button of its own; it points at the turn's Try again, which re-sends", async () => {
  const room = await shown("failed");
  const d = room.one("handoff-divider")!;
  expect(d.querySelectorAll("button")).toHaveLength(1);
  expect(room.one("handoff-again")?.textContent).toBe("Try again sends your question with it.");
  // one Try again on the page: the failed turn's own
  const again = room.all("answer-try-again");
  expect(again).toHaveLength(1);
  await room.click(again[0]);
  await room.settle();
  const sent = room.sent("send_prompt");
  expect(sent).toHaveLength(1);
  expect(sent[0]!.content).toEqual([{ text: { text: "Third question" } }]);
  // no agent named: the thread is on the new agent already, which the host owes the handover
  expect(sent[0]!.agent).toBeUndefined();
});

test("where the thread kept its agent (a handover too long to fit), the line doesn't point at Try again", async () => {
  const room = await shown("failed", "claude-acp");
  expect(room.one("handoff-line")?.textContent).toBe(
    "Context not restored · Claude Opus 5.5 → ChatGPT",
  );
  expect(room.one("handoff-again")).toBeNull();
});

test("the turn's Try again carries it only while the thread is on the new agent, idle, that question the newest", () => {
  const entries = (extra: ViewEntry[] = []) =>
    new Map(
      [...view("failed"), ...extra].map((e) => [Object.keys(e)[0] + ":" + idOf(e), e] as const),
    );
  const idOf = (e: ViewEntry): string => {
    const v = Object.values(e)[0] as Record<string, string>;
    return v["message"] ?? v["turn"] ?? v["handoff"] ?? "";
  };
  const make = (agent: string, status: "idle" | "running", extra: ViewEntry[] = []) => {
    const map = entries(extra);
    const head = headOn(agent);
    head.header!.status = status;
    return { ids: [...map.keys()], entries: map, header: head, texts: new Map() } as never;
  };
  const failed = handoff("failed").handoff!;
  expect(canTryAgain(make("codex-acp", "idle"), failed)).toBe(true);
  // a handover too long to fit commits no switch: the thread kept Claude
  expect(canTryAgain(make("claude-acp", "idle"), failed)).toBe(false);
  expect(canTryAgain(make("codex-acp", "running"), failed)).toBe(false);
  expect(canTryAgain(make("codex-acp", "idle", exchange(4, "Later question")), failed)).toBe(false);
  expect(canTryAgain(make("codex-acp", "idle"), handoff("delivered").handoff!)).toBe(false);
  expect(handoffFor(make("codex-acp", "idle"), uuid(3, 5))).toEqual(failed);
});

test("a reload of the same view draws the divider again, as it was", async () => {
  const room = await shown("failed");
  room.deliver([reset(view("failed"), headOn())]);
  await room.settle();
  expect(room.all("handoff-divider")).toHaveLength(1);
  expect(room.one("handoff-divider")?.dataset["state"]).toBe("failed");
  expect(room.one("handoff-divider")?.nextElementSibling).toBe(room.all("message-user")[2]!);
});

test("an end names Claude or ChatGPT by its kind, and the model only when known", () => {
  expect(endText({ agent: "claude-acp", model: { id: "o", name: "Opus 5.5" } })).toBe(
    "Claude Opus 5.5",
  );
  expect(endText({ agent: "codex-acp" })).toBe("ChatGPT");
  expect(endText({ agent: "fake-agent", model: { id: "x", name: " " } })).toBe("fake-agent");
});
