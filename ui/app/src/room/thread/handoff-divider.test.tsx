// @vitest-environment happy-dom
// The handover's divider in the thread (handoff.ts, handoff-divider.tsx; host #1349's "handoff" entry): it sits just
// before the question it went with, never in its own place; pending and given read as one quiet line; a settled one
// that may not have reached the new agent says so under it (statusLine, one map, by the host's own status): not
// confirmed ("unconfirmed"), quiet with an amber dot and no promise; never sent ("failed"), red with the failed
// mark; a newer status, the next-message sentence alone; that sentence only while the next message will carry it.
// A switch back (resumed) says the agent picks up its own conversation, and with nothing passed on, no Passing on.
// One Try again per failure, the turn's own, re-sends the question. A reload draws it again.
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { HandoffEntryStatus, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { endText, handoffFor, shownOf, standingOf, statusLine } from "./handoff.ts";

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

const handoff = (status: string, leftOut = 2, resumed = false): ViewEntry => ({
  handoff: {
    ...(resumed ? { resumed } : {}),
    order: 35,
    handoff: uuid(1, 12),
    message: uuid(3, 5),
    from: { agent: "claude-acp", model: { id: "opus", name: "Opus 5.5" } },
    to: { agent: "codex-acp" },
    answers_covered: 2,
    items_included: 7,
    items_left_out: leftOut,
    status: status as HandoffEntryStatus,
    ts: 35_000,
  },
});

/** The thread on ChatGPT (the handoff's new agent) unless `agent` says otherwise. */
function headOn(agent = "codex-acp"): ViewEntry {
  const head = header();
  head.header!.agent = agent;
  return head;
}

function view(status: string, leftOut?: number): ViewEntry[] {
  return [
    ...exchange(1, "First question"),
    ...exchange(2, "Second question"),
    ...exchange(3, "Third question", status === "pending" || status === "delivered"),
    handoff(status, leftOut),
  ];
}

async function shown(status: string, agent?: string, leftOut?: number) {
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
  expect(room.one("handoff-status")).toBeNull();
});

test("pending reads the same quiet line with a small Passing on mark, never red", async () => {
  const room = await shown("pending");
  expect(room.one("handoff-divider")?.dataset["state"]).toBe("pending");
  expect(room.one("handoff-line")?.textContent).toBe("Context handoff · Claude Opus 5.5 → ChatGPT");
  expect(room.one("handoff-passing")?.textContent).toBe("Passing on");
  expect(room.one("handoff-divider")?.querySelector(".text-red-text, svg")).toBeNull();
});

test("the host's unconfirmed reads not confirmed: quiet with an amber dot, never red, no promise", async () => {
  const room = await shown("unconfirmed");
  const d = room.one("handoff-divider")!;
  expect(d.dataset["state"]).toBe("unconfirmed");
  expect(room.one("handoff-line")?.textContent).toBe("Context handoff · Claude Opus 5.5 → ChatGPT");
  expect(room.one("handoff-status")?.textContent).toBe(
    "Clauseo couldn't confirm the handover reached ChatGPT.",
  );
  expect(room.one("handoff-amber")).not.toBeNull();
  expect(d.querySelector(".text-red-text, svg")).toBeNull();
});

test("the host's failed on the new agent is red, and the next message carries it", async () => {
  const room = await shown("failed");
  const d = room.one("handoff-divider")!;
  expect(d.dataset["state"]).toBe("failed");
  expect(room.one("handoff-status")?.textContent).toBe(
    "The handover didn't go out. It's sent with your next message.",
  );
  expect(room.one("handoff-amber")).toBeNull();
});

test("a switch back says the agent picks up its own conversation; what it passed on, only what it hadn't seen", async () => {
  h = await mountRoom();
  h.deliver([reset([...view("pending").slice(0, -1), handoff("pending", 0, true)], headOn())]);
  await h.settle();
  expect(h.one("handoff-divider")?.dataset["resumed"]).toBe("");
  expect(h.one("handoff-line")?.textContent).toBe(
    "Back to ChatGPT, which picks up its own conversation",
  );
  expect(h.one("handoff-passing")?.textContent).toBe("Passing on");
  await h.click(h.one("handoff-more"));
  expect(h.one("handoff-passed-on")?.textContent).toBe(
    "Only what ChatGPT hadn't seen. Earlier answers: 2 · Passed on in full: 7",
  );
});

test("a switch back that passes nothing on: no Passing on, nothing to open", async () => {
  const nothing: ViewEntry = handoff("pending", 0, true);
  Object.assign(nothing.handoff!, { answers_covered: 0, items_included: 0 });
  h = await mountRoom();
  h.deliver([reset([...view("pending").slice(0, -1), nothing], headOn())]);
  await h.settle();
  expect(h.one("handoff-line")?.textContent).toBe(
    "Back to ChatGPT, which picks up its own conversation",
  );
  expect(h.one("handoff-passing")).toBeNull();
  expect(h.one("handoff-more")).toBeNull();
});

test("never sent (the thread kept its agent: too long to fit) is red with the failed mark, no next-message promise", async () => {
  const room = await shown("failed", "claude-acp");
  const d = room.one("handoff-divider")!;
  expect(d.dataset["state"]).toBe("failed");
  expect(room.one("handoff-status")?.textContent).toBe("The handover didn't go out.");
  expect(d.querySelector(".text-red-text svg")).not.toBeNull();
});

test("a status from a newer Clauseo: the next-message sentence alone, amber, not an alarm", async () => {
  const room = await shown("brand_new");
  const d = room.one("handoff-divider")!;
  expect(d.dataset["state"]).toBe("unknown");
  expect(room.one("handoff-status")?.textContent).toBe("It's sent with your next message.");
  expect(d.querySelector(".text-red-text, svg")).toBeNull();
});

test("What was passed on opens the counts; none left out says nothing of it", async () => {
  const room = await shown("delivered");
  expect(room.one("handoff-passed-on")).toBeNull();
  await room.click(room.one("handoff-more"));
  expect(room.one("handoff-passed-on")?.textContent).toBe(
    "Earlier answers: 2 · Passed on in full: 7 · Left out or shortened for lack of room: 2",
  );
  await room.unmount();
  h = null;
  const none = await shown("delivered", undefined, 0);
  await none.click(none.one("handoff-more"));
  expect(none.one("handoff-passed-on")?.textContent).toBe(
    "Earlier answers: 2 · Passed on in full: 7",
  );
});

test("one Try again per failure: the turn's own, which re-sends the question", async () => {
  const room = await shown("failed");
  expect(room.one("handoff-divider")!.querySelectorAll("button")).toHaveLength(1);
  const again = room.all("answer-try-again");
  expect(again).toHaveLength(1);
  await room.click(again[0]);
  await room.settle();
  const sent = room.sent("send_prompt");
  expect(sent).toHaveLength(1);
  expect(sent[0]!.content).toEqual([{ text: { text: "Third question" } }]);
  expect(sent[0]!.agent).toBeUndefined();
});

test("statusLine: one map from status to words; the next-message sentence only while it is true", () => {
  const owed = { onNewAgent: true, newest: true };
  const carried = { onNewAgent: true, newest: false };
  const kept = { onNewAgent: false, newest: true };
  const keys = (l: ReturnType<typeof statusLine>) => l.sentences.map((s) => s.key);
  expect(statusLine("delivered", "ChatGPT", owed)).toEqual({ tone: "quiet", sentences: [] });
  expect(statusLine("pending", "ChatGPT", owed).sentences).toEqual([]);
  expect(statusLine("unconfirmed", "ChatGPT", owed).tone).toBe("amber");
  expect(keys(statusLine("unconfirmed", "ChatGPT", carried))).toEqual([
    "Clauseo couldn't confirm the handover reached {agent}.",
  ]);
  expect(statusLine("failed", "ChatGPT", owed)).toEqual({
    tone: "red",
    sentences: [
      { key: "The handover didn't go out." },
      { key: "It's sent with your next message." },
    ],
  });
  expect(keys(statusLine("unknown", "ChatGPT", kept))).toEqual([]);
  // the host's status, straight through
  expect(shownOf("failed")).toBe("failed");
  expect(shownOf("unconfirmed")).toBe("unconfirmed");
  expect(shownOf("brand_new")).toBe("unknown");
});

test("standing: on the new agent, and the handoff's question the newest", () => {
  const make = (agent: string, extra: ViewEntry[] = []) => {
    const all = [...view("failed"), ...extra];
    const idOf = (e: ViewEntry): string => {
      const v = Object.values(e)[0] as Record<string, string>;
      return v["message"] ?? v["turn"] ?? v["handoff"] ?? "";
    };
    const map = new Map(all.map((e) => [Object.keys(e)[0] + ":" + idOf(e), e] as const));
    return { ids: [...map.keys()], entries: map, header: headOn(agent), texts: new Map() } as never;
  };
  const failed = handoff("failed").handoff!;
  expect(standingOf(make("codex-acp"), failed)).toEqual({ onNewAgent: true, newest: true });
  expect(standingOf(make("claude-acp"), failed)).toEqual({ onNewAgent: false, newest: true });
  expect(standingOf(make("codex-acp", exchange(4, "Later question")), failed)).toEqual({
    onNewAgent: true,
    newest: false,
  });
  expect(handoffFor(make("codex-acp"), uuid(3, 5))).toEqual(failed);
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
