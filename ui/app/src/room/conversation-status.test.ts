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
  // a turn the agent ended on the lawyer's refusal (private #361) is stopped, never ready
  expect(unseen("not_allowed")).toEqual({ state: "stopped", at: 42 });
  // nor one the agent ended on Clauseo's refusal of files outside the matter
  expect(unseen("outside_matter")).toEqual({ state: "stopped", at: 42 });
  expect(unseen("failed")).toEqual({ state: "failed", at: 42 });
  // the model's own stops, a turn too long to read among them, are neither the lawyer's stop nor a failure: there is
  // something to read
  for (const reason of ["max_tokens", "refusal", "too_long"] as const)
    expect(unseen(reason)).toEqual({ state: "ready" });
  expect(words("idle", { seen: true, last_stop: { reason: "end_turn", at: 42 } })).toEqual({
    state: "idle",
  });
  expect(words("idle", { seen: true, last_stop: { reason: "cancelled", at: 42 } })).toEqual({
    state: "idle",
  });
});
