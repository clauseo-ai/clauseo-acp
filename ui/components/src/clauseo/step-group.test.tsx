// @vitest-environment happy-dom
// The step group's parts: the icon pass's glyph for each tool kind (other for the rest, the break for a failure), the line a real button that
// opens and folds the rows with aria-expanded, a failure on the line, and a row's paper with its folder on hover.
import { act, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import {
  IconBreak,
  IconToolDelete,
  IconToolEdit,
  IconToolExecute,
  IconToolFetch,
  IconToolMove,
  IconToolOther,
  IconToolRead,
  IconToolSearch,
  IconToolSwitchMode,
  IconToolThink,
} from "../icons/icons.tsx";
import type { IconProps } from "../icons/icon.tsx";
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

/** Each kind and the icon pass's glyph it draws. */
const GLYPHS: readonly [string, (p: IconProps) => ReactNode][] = [
  ["read", IconToolRead],
  ["edit", IconToolEdit],
  ["delete", IconToolDelete],
  ["move", IconToolMove],
  ["search", IconToolSearch],
  ["execute", IconToolExecute],
  ["think", IconToolThink],
  ["fetch", IconToolFetch],
  ["switch_mode", IconToolSwitchMode],
  ["other", IconToolOther],
];
const svg = (Glyph: (p: IconProps) => ReactNode, className?: string) =>
  renderToStaticMarkup(<Glyph size={14} {...(className === undefined ? {} : { className })} />);

test("each tool kind draws its own glyph from the icon pass, hidden from assistive technology; an unknown kind draws other's, a failure the break", () => {
  const drawn = GLYPHS.map(([kind]) =>
    renderToStaticMarkup(<ToolKindIcon kind={kind} size={14} />),
  );
  expect(drawn).toEqual(GLYPHS.map(([, Glyph]) => svg(Glyph)));
  expect(new Set(drawn).size).toBe(GLYPHS.length);
  for (const one of drawn) {
    expect(one).toContain('aria-hidden="true"');
    expect(one).toContain('stroke-width="1.5"');
  }
  expect(renderToStaticMarkup(<ToolKindIcon kind="unknown" size={14} />)).toBe(svg(IconToolOther));
  const failed = renderToStaticMarkup(<ToolKindIcon kind="read" failed size={14} />);
  expect(failed).toBe(svg(IconBreak));
  expect(new Set([...drawn, failed]).size).toBe(GLYPHS.length + 1);
  // a failed row draws the break in its kind's place
  const row = (status: string) =>
    renderToStaticMarkup(
      <StepRow
        id="tool_call:1"
        toolCall="1"
        status={status}
        kind="read"
        text="Read Lease.pdf."
        hover={null}
        mark={null}
      />,
    );
  expect(row("failed")).toContain(svg(IconBreak, "step-icon"));
  expect(row("completed")).toContain(svg(IconToolRead, "step-icon"));
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
          text="Tried to read Lease.pdf."
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
  // the failure on the line is led by the break, not only its weight
  const trouble = host.querySelector('[data-testid="step-group-trouble"]')!;
  expect(trouble.querySelector(".step-inline-icon.step-trouble-icon > svg")?.outerHTML).toBe(
    svg(IconBreak),
  );
  // the chevron follows the last word inside the words, so it stays beside them when the line wraps
  const words = host.querySelector(".step-group-words")!;
  expect(words.lastElementChild?.classList.contains("step-group-chev")).toBe(true);
  expect(words.lastElementChild?.classList.contains("step-inline-icon")).toBe(true);
  expect(host.querySelector('[data-testid="tool-call"]')).toBeNull();
  await act(async () => line.click());
  expect(line.getAttribute("aria-expanded")).toBe("true");
  const rows = host.querySelector("ol")!;
  expect(line.getAttribute("aria-controls")).toBe(rows.id);
  const row = rows.querySelector<HTMLElement>('[data-testid="tool-call"]')!;
  expect(row.textContent).toBe("Tried to read Lease.pdf. · Failed");
  expect(row.querySelector("[title]")?.getAttribute("title")).toBe("papers/2019");
  await act(async () => line.click());
  expect(host.querySelector("ol")).toBeNull();
});

test("a group whose every step is over with none finished says what was tried once, as its trouble, led by the break alone", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <StepGroup
        id="tool_call:1"
        kinds={[]}
        line=""
        trouble="Tried to read indiankanoon.org online."
      >
        {null}
      </StepGroup>,
    ),
  );
  const line = host.querySelector("button")!;
  expect(line.textContent).toBe("Tried to read indiankanoon.org online.");
  expect(host.querySelector(".step-group-now")).toBeNull();
  expect(host.querySelector('[data-testid="step-group-trouble"] svg')).not.toBeNull();
  // no icons before the words: the break is the line's first glyph, the chevron its only other
  expect(host.querySelector(".step-group-icons")).toBeNull();
  const glyphs = [...line.querySelectorAll("svg")];
  expect(glyphs).toHaveLength(2);
  expect(glyphs[0]!.closest(".step-trouble-icon")).not.toBeNull();
});
