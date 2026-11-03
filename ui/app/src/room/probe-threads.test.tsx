// @vitest-environment happy-dom
// The probe build's ?thread=permission page (probe-main.tsx): the room lists the turn's thread through next_query
// (probe-threads.ts), subscribes to it, and shows the pending prompt with the composer busy (Stop, not Ask). And its
// ?thread=shaping page: the idle thread, whose answer (testing/shaping-answer.ts) renders every Devanagari shaping cell,
// verbatim and NFC, in every block type.
import { afterEach, expect, test, vi } from "vitest";
import { SHAPING_ANSWER, SHAPING_CELLS, SHAPING_QUESTION } from "../testing/shaping-answer.ts";
import {
  agentText,
  chosen,
  editCall,
  header,
  permission,
  reset,
  STREAM,
  THREAD,
  upsert,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { ThreadInfo } from "./host-next.ts";
import { probeThreads } from "./probe-threads.ts";
import { mountRoom, THE_MATTER, type Harness } from "./thread/harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

const ASKING = { asking: true, commanding: false };
const COMMANDING = { asking: false, commanding: true };
const SHAPING = { asking: false, commanding: false, shaping: true };

test("the probe's threads: ?thread=permission lists the turn's thread waiting on the lawyer, ?thread=commands its idle one, neither the page's own", () => {
  const own: ThreadInfo[] = [
    { thread: uuid(5, 1), matter: THE_MATTER.matter, agent: "codex", status: "idle", seen: true },
  ];
  expect(probeThreads(ASKING, own)).toEqual([
    {
      thread: THREAD,
      matter: THE_MATTER.matter,
      agent: "claude-acp",
      status: "needs_you",
      seen: false,
    },
  ]);
  expect(probeThreads(COMMANDING, own)).toEqual([
    { thread: THREAD, matter: THE_MATTER.matter, agent: "claude-acp", status: "idle", seen: true },
  ]);
  expect(probeThreads({ asking: false, commanding: false }, own)).toBe(own);
});

test("the probe's threads: ?thread=shaping lists the idle thread, not the page's own", () => {
  const own: ThreadInfo[] = [
    { thread: uuid(5, 1), matter: THE_MATTER.matter, agent: "codex", status: "idle", seen: true },
  ];
  expect(probeThreads(SHAPING, own)).toEqual([
    { thread: THREAD, matter: THE_MATTER.matter, agent: "claude-acp", status: "idle", seen: true },
  ]);
});

test("?thread=shaping: the room subscribes to the idle thread and renders every cell, NFC, in every block type", async () => {
  h = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER)],
      threads: () => probeThreads(SHAPING, []),
    }),
  });
  expect(h.transport.only("subscribe").map((c) => c.stream)).toEqual([STREAM]);
  // the thread's entries as the probe streams them
  h.deliver([
    reset([userMessage(2, 1, SHAPING_QUESTION), agentText(3, 100, SHAPING_ANSWER)], header("idle")),
  ]);
  await h.settle();
  const answer = h.one("message-agent");
  expect(answer, "the shaping answer is rendered").not.toBeNull();
  if (answer === null) return;
  const text = answer.textContent ?? "";
  // no Markdown syntax reaches the screen
  for (const raw of ["**", "|", "```", "## ", "> ", "- "])
    expect(text.includes(raw), `raw Markdown ${JSON.stringify(raw)} on screen`).toBe(false);
  // every block type is there, and each one, taken over the answer, holds every cell verbatim and NFC
  const blocks: [string, string][] = [
    ["h2 heading", "h2"],
    ["paragraph", "p"],
    ["bulleted list", "ul > li"],
    ["table", "table"],
    ["blockquote", "blockquote"],
    ["inline code", ":not(pre) > code"],
    ["code block", "pre"],
    ["bold", "strong"],
    ["italic", "em"],
  ];
  for (const [kind, selector] of blocks) {
    const found = [...answer.querySelectorAll(selector)];
    expect(found.length, `no ${kind} (${selector}) in the answer`).toBeGreaterThan(0);
    for (const cell of SHAPING_CELLS) {
      const holder = found.find((el) => (el.textContent ?? "").includes(cell.text));
      expect(holder, `the ${cell.name} cell is missing from the ${kind}`).toBeDefined();
      const where = holder?.textContent ?? "";
      expect(cell.text.normalize("NFC"), `the ${cell.name} cell is not NFC`).toBe(cell.text);
      expect(where.normalize("NFC"), `the ${kind} holding the ${cell.name} cell is not NFC`).toBe(
        where,
      );
    }
  }
  // the table: a header row and two body rows in each section
  for (const table of answer.querySelectorAll("table")) {
    expect(table.querySelectorAll("thead tr").length).toBe(1);
    expect(table.querySelectorAll("tbody tr").length).toBe(2);
  }
}, 30_000);

test("?thread=permission: the room subscribes to the turn's thread and shows its pending prompt with the composer busy", async () => {
  h = await mountRoom({
    next: fakeNext({
      matters: () => [detailed(THE_MATTER)],
      threads: () => probeThreads(ASKING, []),
    }),
  });
  // the room asks the host for the matter's threads and subscribes to the one it lists
  expect(h.transport.only("subscribe").map((c) => c.stream)).toEqual([STREAM]);
  // the turn's entries as the probe streams them: a pending prompt above one answered once
  h.deliver([
    reset([userMessage(2, 1, "Tighten the notice to the bank.")], header("needs_you")),
    upsert(editCall(100, 301, "pending")),
    upsert(permission(101, 301)),
    upsert(editCall(102, 302, "completed")),
    upsert(permission(103, 302, chosen("allow_once"))),
  ]);
  const prompts = h.all("permission").map((p) => p.dataset["state"]);
  expect(prompts).toEqual(["pending", "resolved"]);
  // busy: Stop stands where Ask does
  expect(h.one("cancel")).not.toBeNull();
  expect(h.one("send")).toBeNull();
}, 30_000);
