// @vitest-environment happy-dom
// The gallery's step group samples say what the room says: the failed sample's row tells what it tried, in the past,
// and its line counts only the steps that finished, with their icons (steps.ts restOf and doneOf).
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { lineOf, doneOf, hoverOf } from "../../room/thread/steps.ts";
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

const FOLDER = "/Users/x/Matters/Sharma";
const raw = (input: object) => ({ raw_input: { inline: JSON.stringify(input) } });

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
    call(1, "read", "completed", raw({ file_path: `${FOLDER}/Complaint.pdf` })),
    call(2, "search", "completed", raw({ pattern: "S.M.S. Pharmaceuticals" })),
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

test("the opened sample's rows read as the room reads the same calls: each paper by its file name, its folder on hover, the search by its words", async () => {
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
    h.textContent?.includes("on the meta line, opened under it"),
  )!.parentElement!;
  const calls = [
    call(1, "read", "completed", raw({ file_path: `${FOLDER}/Complaint.pdf` })),
    call(2, "search", "completed", raw({ pattern: "S.M.S. Pharmaceuticals" })),
    call(3, "read", "completed", raw({ file_path: `${FOLDER}/Form DIR-12.pdf` })),
    call(4, "edit", "completed", raw({ file_path: `${FOLDER}/Reply to the complaint.docx` })),
  ];
  const rows = [...sample.querySelectorAll<HTMLElement>('[data-testid="tool-call"]')];
  expect(rows.map((r) => r.textContent)).toEqual(calls.map((c) => doneOf("en", c)));
  expect(rows.map((r) => r.querySelector("[title]")?.getAttribute("title") ?? null)).toEqual(
    calls.map(hoverOf),
  );
  const room = lineOf(
    "en",
    calls.map((c) => ({ tool_call: c })),
    false,
  );
  // on the meta line: the name and the time, then the line the room makes of the same calls
  const meta = sample.querySelector(".meta")!;
  expect(meta.querySelector(".who")?.textContent).toBe("Clauseo, 4:31 pm");
  expect(meta.querySelector("button")?.textContent).toBe(room.line);
});
