// @vitest-environment happy-dom
// The probe build's ?thread=permission page (probe-main.tsx): the room lists the turn's thread through next_query
// (probe-threads.ts), subscribes to it, and shows the pending prompt with the composer busy (Stop, not Ask).
import { afterEach, expect, test, vi } from "vitest";
import {
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
