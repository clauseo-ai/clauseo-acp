// @vitest-environment happy-dom
// The step group's parts: an icon of its own for each tool kind (other for the rest), the line a real button that
// opens and folds the rows with aria-expanded, a failure on the line, and a row's paper with its folder on hover.
import { act } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { StepGroup, StepRow } from "./step-group.tsx";
import { ToolKindIcon } from "./tool-kind-icons.tsx";
import { stepKindOf } from "./tool-kinds.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  host = root = null;
});

const KINDS = [
  "read",
  "edit",
  "delete",
  "move",
  "search",
  "execute",
  "think",
  "fetch",
  "other",
] as const;

test("each tool kind draws its own icon, hidden from assistive technology; any other kind draws other's", () => {
  const drawn = KINDS.map((kind) => renderToStaticMarkup(<ToolKindIcon kind={kind} size={14} />));
  expect(new Set(drawn).size).toBe(KINDS.length);
  for (const svg of drawn) {
    expect(svg).toContain('aria-hidden="true"');
    expect(svg).toContain('stroke-width="1.5"');
  }
  const other = renderToStaticMarkup(<ToolKindIcon kind="other" size={14} />);
  expect(renderToStaticMarkup(<ToolKindIcon kind="switch_mode" size={14} />)).toBe(other);
  expect(renderToStaticMarkup(<ToolKindIcon kind="unknown" size={14} />)).toBe(other);
  expect(stepKindOf("fetch")).toBe("fetch");
  expect(stepKindOf("switch_mode")).toBe("other");
});

test("the line is a button that opens its rows in place and folds them, with aria-expanded and aria-controls", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <StepGroup
        id="tool_call:1"
        kinds={["read", "search"]}
        line="2 papers read. Searched once."
        trouble="1 of them failed."
      >
        <StepRow
          id="tool_call:1"
          toolCall="1"
          status="failed"
          kind="read"
          text="Reading Lease.pdf."
          hover="papers/2019"
          mark="Failed"
        />
      </StepGroup>,
    ),
  );
  const line = host.querySelector("button")!;
  expect(line.getAttribute("aria-expanded")).toBe("false");
  expect(line.textContent).toBe("2 papers read. Searched once. 1 of them failed.");
  expect(host.querySelector('[data-testid="step-group"]')?.getAttribute("data-trouble")).toBe(
    "true",
  );
  expect(host.querySelectorAll(".step-group-icons svg")).toHaveLength(2);
  expect(host.querySelector('[data-testid="tool-call"]')).toBeNull();
  await act(async () => line.click());
  expect(line.getAttribute("aria-expanded")).toBe("true");
  const rows = host.querySelector("ol")!;
  expect(line.getAttribute("aria-controls")).toBe(rows.id);
  const row = rows.querySelector<HTMLElement>('[data-testid="tool-call"]')!;
  expect(row.textContent).toBe("Reading Lease.pdf. · Failed");
  expect(row.querySelector("[title]")?.getAttribute("title")).toBe("papers/2019");
  await act(async () => line.click());
  expect(host.querySelector("ol")).toBeNull();
});
