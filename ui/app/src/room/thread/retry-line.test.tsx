// @vitest-environment happy-dom
// The working line while the agent waits to retry its service (retry-line.ts, host #1335), through the room over
// FakeTransport: the header's retrying set says the plan's servers are busy in Clauseo's own words, never the
// agent's title or details and no number; cleared, "Answering." comes back; a room opened while it is set says it at
// once; and ChatGPT's thread names ChatGPT.
import { assertWarm, WARM_MS, warmUp } from "@clauseo/components/lib/warm-up.ts";
import { afterEach, beforeAll, beforeEach, expect, test } from "vitest";
import { agentText, header, reset, upsert, userMessage } from "../../testing/stream-script.ts";
import type { AgentRetry, ViewEntry } from "../../transport/protocol.ts";
import { loadThread } from "./chunk.ts";
import { mountRoom, type Harness } from "./harness.ts";

let warmFailure: string | null = null;
beforeAll(async () => {
  warmFailure = await warmUp("the thread's chunk", loadThread);
}, WARM_MS + 5_000);

let h: Harness | null = null;
beforeEach(() => assertWarm(warmFailure));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const CLAUDE_BUSY = "Claude's servers are busy. It's trying again.";
const RETRY: AgentRetry = {
  since: 1_700_000_000_000,
  title: "Retrying Claude, attempt 2 of 10.",
  details: "HTTP 529 (overloaded). Next attempt in 8s.",
};
const ASKED = [
  userMessage(5, 1, "Summarise the notice to the bank."),
  agentText(6, 1, "Clause nine"),
];

/** The running thread's header, waiting to retry when `retrying` is given; its agent Claude's unless said. */
const running = (retrying?: AgentRetry, agent?: string): ViewEntry => {
  const head = header();
  head.header!.status = "running";
  if (agent !== undefined) head.header!.agent = agent;
  if (retrying !== undefined) head.header!.retrying = retrying;
  return head;
};
const line = () => h!.one("working")?.textContent ?? "";

test("retrying set on the running answer: the busy line, without the agent's words or any number", async () => {
  h = await mountRoom();
  h.deliver([reset(ASKED, running())]);
  await h.settle();
  expect(line()).toContain("Answering.");
  h.deliver([upsert(running(RETRY))]);
  await h.settle();
  expect(line()).toBe(CLAUDE_BUSY);
  expect(h.host.textContent).not.toContain("Retrying Claude");
  expect(h.host.textContent).not.toContain("HTTP 529");
  expect(line()).not.toMatch(/\d/);
  // a later attempt's words change nothing the lawyer sees
  h.deliver([upsert(running({ ...RETRY, title: "Retrying Claude, attempt 3 of 10." }))]);
  await h.settle();
  expect(line()).toBe(CLAUDE_BUSY);
});

test("retrying cleared: the ordinary line comes back", async () => {
  h = await mountRoom();
  h.deliver([reset(ASKED, running(RETRY))]);
  await h.settle();
  expect(line()).toBe(CLAUDE_BUSY);
  h.deliver([upsert(running())]);
  await h.settle();
  expect(line()).toContain("Answering.");
  expect(line()).not.toContain("busy");
});

test("a reload while retrying is set: the reopened room says it at once", async () => {
  h = await mountRoom();
  h.deliver([reset(ASKED, running(RETRY))]);
  await h.settle();
  expect(line()).toBe(CLAUDE_BUSY);
  await h.unmount();
  document.body.replaceChildren();
  h = await mountRoom();
  h.deliver([reset(ASKED, running(RETRY))]);
  await h.settle();
  expect(line()).toBe(CLAUDE_BUSY);
});

test("ChatGPT's thread names ChatGPT", async () => {
  h = await mountRoom();
  const codex = {
    ...RETRY,
    title: "Reconnecting... 1/5",
    details: "Selected model is at capacity.",
  };
  h.deliver([reset(ASKED, running(codex, "codex-acp"))]);
  await h.settle();
  expect(line()).toBe("ChatGPT's servers are busy. It's trying again.");
  expect(h.host.textContent).not.toContain("Reconnecting");
});
