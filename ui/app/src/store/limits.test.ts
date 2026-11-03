// The limits store (U12): it changes only when a view arrives and only for an agent whose entry changed, keeping an
// unchanged entry's object; refresh sends refresh_limits, naming the agent or, for every agent, none.
import { expect, test, vi } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { NullTransport } from "../transport/host-transport.ts";
import type { AgentLimitsView } from "../transport/protocol.ts";
import { limitsStoreOf } from "./limits.ts";

const entry = (freshness: AgentLimitsView["freshness"]): AgentLimitsView => ({
  billing: "plan",
  windows: [],
  at_limit_text: "",
  freshness,
});

test("a view that changes one agent keeps the other's object, and a same view tells no one", () => {
  const t = new FakeTransport();
  const store = limitsStoreOf(t);
  expect(limitsStoreOf(t)).toBe(store);
  const heard = vi.fn();
  store.subscribe(heard);
  expect(store.view()).toEqual({});
  t.limitsChanged({ claude: entry("loading"), codex: entry("loading") });
  expect(heard).toHaveBeenCalledTimes(1);
  const codex = store.agent("codex");
  t.limitsChanged({ claude: entry("fresh"), codex: entry("loading") });
  expect(heard).toHaveBeenCalledTimes(2);
  expect(store.agent("codex")).toBe(codex);
  expect(store.agent("claude")?.freshness).toBe("fresh");
  const view = store.view();
  t.limitsChanged({ claude: entry("fresh"), codex: entry("loading") });
  expect(heard).toHaveBeenCalledTimes(2);
  expect(store.view()).toBe(view);
  // an agent gone from the view is a change
  t.limitsChanged({ claude: entry("fresh") });
  expect(heard).toHaveBeenCalledTimes(3);
  expect(store.agent("codex")).toBeUndefined();
});

test("a store made after the first view starts from it", () => {
  const t = new FakeTransport();
  t.limitsChanged({ codex: entry("stale") });
  expect(limitsStoreOf(t).agent("codex")?.freshness).toBe("stale");
});

test("refresh sends refresh_limits with the agent, or none for every agent", async () => {
  const t = new FakeTransport();
  const store = limitsStoreOf(t);
  store.refresh("claude", "shown");
  store.refresh(undefined, "try_again");
  await Promise.resolve();
  expect(t.only("dispatch").map((c) => c.command.command)).toEqual([
    { refresh_limits: { agent: "claude", why: "shown" } },
    { refresh_limits: { why: "try_again" } },
  ]);
});

test("a transport that carries no limits gives an empty store", () => {
  expect(limitsStoreOf(new NullTransport()).view()).toEqual({});
});
