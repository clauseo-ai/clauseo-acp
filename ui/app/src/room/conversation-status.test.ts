// rowWords in conversation-status.ts: a row's words for every thread status, and H6's marks over idle.
import { expect, test } from "vitest";
import type { ThreadStatus } from "../transport/protocol.ts";
import { rowWords } from "./conversation-status.ts";
import type { ConversationRow } from "./conversations.ts";
import { T2 } from "./conversations-rig.ts";

test("rowWords for every status, and H6's marks over idle", () => {
  const words = (status: ThreadStatus, over: Partial<ConversationRow> = {}) =>
    rowWords({ status, ...over });
  expect(words("starting")).toEqual({ state: "working" });
  expect(words("running")).toEqual({ state: "working" });
  expect(words("needs_you")).toEqual({ state: "question" });
  expect(words({ queued: { behind: T2 } })).toEqual({ state: "waiting", behind: T2 });
  expect(words("idle")).toEqual({ state: "idle" });
  expect(words({ paused_at_limit: { resets_at: 5 } })).toEqual({ state: "idle" });
  expect(words("agent_exited")).toEqual({ state: "idle" });
  expect(words("idle_stopped")).toEqual({ state: "idle" });
  const unseen = (reason: NonNullable<ConversationRow["last_stop"]>["reason"]) =>
    words("idle", { seen: false, last_stop: { reason, at: 42 } });
  expect(unseen("end_turn")).toEqual({ state: "ready" });
  expect(unseen("cancelled")).toEqual({ state: "stopped", at: 42 });
  expect(unseen("failed")).toEqual({ state: "failed", at: 42 });
  expect(words("idle", { seen: true, last_stop: { reason: "end_turn", at: 42 } })).toEqual({
    state: "idle",
  });
  expect(words("idle", { seen: true, last_stop: { reason: "cancelled", at: 42 } })).toEqual({
    state: "idle",
  });
});
