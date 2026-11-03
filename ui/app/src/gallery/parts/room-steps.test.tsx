// @vitest-environment happy-dom
// The gallery's step group samples say what the room says: the failed sample's row tells what it tried, in the past,
// and its line counts only the steps that finished, with their icons (steps.ts restOf and doneOf).
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { lineOf, doneOf } from "../../room/thread/steps.ts";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry } from "../../transport/protocol.ts";
import { StepSamples } from "./room-steps.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | undefined;
afterEach(() => {
  act(() => root?.unmount());
  document.body.replaceChildren();
});

const call = (
  n: number,
  kind: ToolCallEntry["kind"],
  status: ToolCallEntry["status"],
  over: Partial<ToolCallEntry> = {},
) => ({ ...toolCall(n, n, status).tool_call!, kind, title: "", ...over }) as ToolCallEntry;

test("the failed sample's line and row are the room's own", async () => {
  const container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  await act(async () =>
    root?.render(
      <PartsRoot>
        <StepSamples />
      </PartsRoot>,
    ),
  );
  const sample = [...container.querySelectorAll("h3")].find((h) =>
    h.textContent?.includes("a failure kept on its line"),
  )!.parentElement!;
  const group = sample.querySelector<HTMLElement>('[data-testid="step-group"]')!;
  // what the room makes of the same steps: a read and a search done, a read online failed
  const calls = [
    call(1, "read", "completed", {
      locations: [{ inside: { path: "papers/2019/Lease deed.pdf" } }],
    }),
    call(2, "search", "completed", {
      raw_input: { inline: JSON.stringify({ pattern: "notice period" }) },
    }),
    call(3, "fetch", "failed", {
      raw_input: { inline: JSON.stringify({ url: "https://indiankanoon.org/doc/1/" }) },
    }),
  ];
  const room = lineOf(
    "en",
    calls.map((c) => ({ tool_call: c })),
    false,
  );
  expect(group.querySelector("button")?.textContent).toBe(`${room.line} ${room.trouble}`);
  expect(group.querySelectorAll(".step-group-icons svg")).toHaveLength(room.kinds.length);
  const rows = [...group.querySelectorAll('[data-testid="tool-call"]')];
  expect(rows.at(-1)?.textContent).toBe(`${doneOf("en", calls[2]!)} · Failed`);
});
