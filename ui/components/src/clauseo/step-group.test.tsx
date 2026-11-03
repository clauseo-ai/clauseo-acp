// @vitest-environment happy-dom
// The step group's parts: the icon pass's glyph for each tool kind (other for the rest, the break for a failure), the line a real button that
// opens and folds the rows with aria-expanded, a failure on the line, and a row's paper with its folder on hover.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import {
  IconBreak,
  IconNeedsPermission,
  IconQuestion,
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
        trouble="1 failed."
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
  // one control, as the design's chip: the lead icons, the words and the chevron all inside the button, so its focus
  // ring and its hover well take in the whole line
  expect(host.querySelectorAll("button")).toHaveLength(1);
  expect(line.firstElementChild?.classList.contains("step-group-icons")).toBe(true);
  expect(line.querySelectorAll(".step-group-icons svg")).toHaveLength(2);
  expect(line.querySelector(".step-group-words")?.parentElement).toBe(line);
  expect(line.querySelector(".step-group-chev svg")).not.toBeNull();
  expect(line.textContent).toBe("2 papers read. Searched once. 1 failed.");
  expect(host.querySelector('[data-testid="step-group"]')?.getAttribute("data-trouble")).toBe(
    "true",
  );
  expect(host.querySelectorAll(".step-group-icons svg")).toHaveLength(2);
  // the failure on the line is led by the break, not only its weight
  const trouble = host.querySelector('[data-testid="step-group-trouble"]')!;
  expect(trouble.querySelector(".step-inline-icon.step-trouble-icon > svg")?.outerHTML).toBe(
    svg(IconBreak),
  );
  // the chevron is held to the last word, inside the words, so it stays beside them when the line wraps; the break to
  // the trouble's first
  const chev = host.querySelector(".step-group-words .step-group-chev.step-inline-icon")!;
  expect(chev.parentElement?.className).toBe("glued");
  expect(chev.parentElement?.textContent).toBe("failed.");
  expect(chev.parentElement?.lastElementChild).toBe(chev);
  expect(trouble.querySelector(".step-trouble-icon")?.parentElement?.textContent).toBe("1");
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

test("on the meta line nothing is cut: every word on screen in the words that wrap, at most three kind icons, the chevron held to the last word, the whole summary its name and no hover copy", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const head = (line: ReactElement) => <div className="meta">{line}</div>;
  const words = "2 दस्तावेज़ पढ़े गए। एक बार खोजा गया। 1 दस्तावेज़ बदला गया। और 2 काम।";
  await act(async () =>
    root!.render(
      <StepGroup
        id="tool_call:1"
        kinds={["read", "search", "edit", "fetch", "move"]}
        line={words}
        more="उनमें से 1 पृष्ठभूमि में चलता रहा।"
        trouble="1 विफल रहा।"
        head={head}
      >
        {null}
      </StepGroup>,
    ),
  );
  const line = host.querySelector<HTMLButtonElement>(".meta > button")!;
  const full = `${words} उनमें से 1 पृष्ठभूमि में चलता रहा। 1 विफल रहा।`;
  // its name, read from its words, all on screen: nothing dropped, no "…", no title
  expect(line.textContent).toBe(full);
  expect(line.hasAttribute("title")).toBe(false);
  expect(line.querySelector(".step-part-dropped, .step-part-cut, .step-group-kept")).toBeNull();
  expect(line.querySelectorAll(".step-group-icons svg")).toHaveLength(3);
  // the aside and the trouble among the words, as off the meta line; the chevron held to the very last word
  const all = line.querySelector(".step-group-words")!;
  expect(all.querySelector(".step-group-more")).not.toBeNull();
  expect(all.querySelector(".step-group-trouble")).not.toBeNull();
  const chev = all.querySelector(".step-group-chev")!;
  expect(chev.parentElement?.className).toBe("glued");
  expect(chev.parentElement?.textContent).toBe("रहा।");
  expect(line.querySelectorAll(".step-group-chev")).toHaveLength(1);
});

test("off the meta line the line keeps no title and its chevron is held to the last word, whichever part ends the words", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  const cases: [Partial<{ line: string; more: string; trouble: string }>, string][] = [
    [{ line: "1 paper read." }, "read."],
    [{ line: "Reading Lease.pdf.", more: "2 done so far." }, "far."],
    [{ line: "", trouble: "Tried to read indiankanoon.org online." }, "online."],
  ];
  for (const [words, tail] of cases) {
    await act(async () =>
      root!.render(
        <StepGroup id="tool_call:1" kinds={["read"]} line={words.line ?? ""} {...words}>
          {null}
        </StepGroup>,
      ),
    );
    const line = host.querySelector("button")!;
    expect(line.hasAttribute("title")).toBe(false);
    const chev = line.querySelectorAll(".step-group-chev");
    expect(chev).toHaveLength(1);
    expect(chev[0]!.parentElement?.className).toBe("glued");
    expect(chev[0]!.parentElement?.textContent).toBe(tail);
    expect(chev[0]!.nextSibling).toBeNull();
  }
});

test("live, a step waiting on the lawyer leads the line with the permission's shield or the question mark, never a kind icon", async () => {
  for (const [asks, Glyph] of [
    ["permission", IconNeedsPermission],
    ["question", IconQuestion],
  ] as const) {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () =>
      root!.render(
        <StepGroup id="tool_call:1" kinds={[]} asks={asks} line="Needs your permission." live>
          {null}
        </StepGroup>,
      ),
    );
    const icons = host.querySelectorAll(".step-group-icons svg");
    expect(icons).toHaveLength(1);
    expect(icons[0]!.outerHTML).toBe(svg(Glyph, "step-icon"));
    await act(async () => root?.unmount());
    host.remove();
  }
  host = root = null;
});

test("a row's clock time stands at its end in a <time>, its digits tabular (steps.css); none when the caller gives none", () => {
  const row = (time?: { text: string; iso: string } | null) => {
    const el = document.createElement("ol");
    el.innerHTML = renderToStaticMarkup(
      <StepRow
        id="tool_call:1"
        toolCall="1"
        status="completed"
        kind="read"
        text="Read Lease.pdf."
        hover={null}
        mark={null}
        {...(time === undefined ? {} : { time })}
      />,
    );
    return el.querySelector("li")!;
  };
  const timed = row({ text: "4:32 pm", iso: "2026-11-04T11:02:00.000Z" });
  const time = timed.lastElementChild;
  expect(time?.tagName).toBe("TIME");
  expect(time?.className).toBe("step-row-time");
  expect(time?.getAttribute("datetime")).toBe("2026-11-04T11:02:00.000Z");
  expect(time?.textContent).toBe("4:32 pm");
  expect(row(null).querySelector("time")).toBeNull();
  expect(row().querySelector("time")).toBeNull();
  // the class carries the tabular figures, and the row a third column for it
  const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "steps.css"), "utf8");
  const rule = /\.step-row-time\s*\{([^}]*)\}/.exec(css)?.[1] ?? "";
  expect(rule).toMatch(/font-variant-numeric:\s*tabular-nums/);
  expect(rule).toMatch(/white-space:\s*nowrap/);
  expect(
    /\.step-row\s*\{[^}]*grid-template-columns:\s*auto minmax\(0, 1fr\) auto/.exec(css),
  ).not.toBeNull();
});
