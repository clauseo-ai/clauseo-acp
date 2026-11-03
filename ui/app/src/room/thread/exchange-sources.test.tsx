// @vitest-environment happy-dom
// An exchange's web sources subscribe only when it has tool calls: an exchange with none renders no sources part and
// never runs the tool calls' picker, and one with tool calls still shows its sources, its picker given the tool call
// entries' ids alone.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { agentText, reset, step, userMessage } from "../../testing/stream-script.ts";
import { eventsOf, headerOf, searched, webCallsFor } from "../../testing/web-sources.ts";
import { mountRoom, type Harness } from "./harness.ts";

const spies = vi.hoisted(() => ({
  sources: 0,
  picks: 0,
  tools: [] as string[][],
}));

vi.mock("./sources.tsx", async (original) => {
  const real = await original<typeof import("./sources.tsx")>();
  return {
    ...real,
    WebSources: (props: Parameters<typeof real.WebSources>[0]) => {
      spies.sources += 1;
      return real.WebSources(props);
    },
  };
});

vi.mock("./sources.ts", async (original) => {
  const real = await original<typeof import("./sources.ts")>();
  return {
    ...real,
    toolCallsPick: (...args: Parameters<typeof real.toolCallsPick>) => {
      spies.tools.push([...args[0]]);
      const pick = real.toolCallsPick(...args);
      return (v: Parameters<typeof pick>[0]) => {
        spies.picks += 1;
        return pick(v);
      };
    },
  };
});

let h: Harness | null = null;
beforeEach(() => {
  spies.sources = 0;
  spies.picks = 0;
  spies.tools = [];
});
afterEach(async () => {
  await h?.unmount();
  h = null;
});

test("an exchange with no tool calls renders no sources part and never runs the picker", async () => {
  h = await mountRoom();
  h.deliver([reset([userMessage(5, 1, "Q?"), agentText(6, 1, "A."), step(9)])]);
  await h.settle();
  expect(h.one("message-agent")).not.toBeNull();
  expect(spies.sources).toBe(0);
  expect(spies.picks).toBe(0);
  expect(spies.tools).toEqual([]);
});

test("an exchange with tool calls still shows its sources, the picker given tool call ids alone", async () => {
  const envelopes = [searched(1, "notice period")];
  h = await mountRoom();
  h.deliver([
    reset(
      [
        userMessage(5, 1, "What is the notice period?"),
        ...webCallsFor(envelopes, "claude-acp"),
        agentText(40, 1, "Thirty days."),
        step(50),
      ],
      headerOf("claude-acp"),
    ),
  ]);
  await act(async () => h!.transport.deliver(eventsOf(envelopes)));
  expect(h.one("web-sources")).not.toBeNull();
  expect(spies.sources).toBeGreaterThan(0);
  expect(spies.picks).toBeGreaterThan(0);
  expect(spies.tools.length).toBeGreaterThan(0);
  for (const tools of spies.tools) {
    expect(tools.length).toBeGreaterThan(0);
    for (const id of tools) expect(id.startsWith("tool_call:")).toBe(true);
  }
});
