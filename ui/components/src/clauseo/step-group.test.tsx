// @vitest-environment happy-dom
// The step group's parts: the icon pass's glyph for each tool kind (other for the rest, the break for a failure), the line a real button that
// opens and folds the rows with aria-expanded, a failure on the line, and a row's paper with its folder on hover.
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

test("on the meta line the line keeps one line: at most three kind icons, the chevron after the words so an ellipsis never cuts it, the whole summary its name and its title", async () => {
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
  // its name, read from its words, all still in the button however the ellipsis cuts them; the hover says it too
  expect(line.textContent).toBe(full);
  expect(line.title).toBe(full);
  expect(line.querySelectorAll(".step-group-icons svg")).toHaveLength(3);
  // the chevron after the words, the button's last part, not among the words
  expect(line.lastElementChild?.classList.contains("step-group-chev")).toBe(true);
  expect(line.querySelector(".step-group-words .step-group-chev")).toBeNull();
});

test("off the meta line the line keeps no title and its chevron follows the last word", async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root!.render(
      <StepGroup id="tool_call:1" kinds={["read"]} line="1 paper read.">
        {null}
      </StepGroup>,
    ),
  );
  const line = host.querySelector("button")!;
  expect(line.hasAttribute("title")).toBe(false);
  expect(line.querySelector(".step-group-words")?.lastElementChild?.classList).toContain(
    "step-group-chev",
  );
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

test("on the meta line, sentences that don't fit drop from the end at a sentence with '…', the aside and the trouble kept, the whole summary still the name; measured again when the meta line's width changes", async () => {
  // a 5 px per character layout: the words' box is `room` wide, each part as wide as its text
  let room = 300;
  const PX = 5;
  // the "…" is drawn by CSS: two characters
  const wide = (e: Element) =>
    e.classList.contains("step-part-cut") ? 2 * PX : (e.textContent ?? "").length * PX;
  const shown = (el: Element) =>
    [
      ...el.querySelectorAll(
        ".step-part:not(.step-part-dropped), .step-part-cut[data-cut], .step-group-more, .step-group-trouble",
      ),
    ].reduce((sum, e) => sum + wide(e), 0);
  const proto = HTMLElement.prototype;
  const saved = {
    scroll: Object.getOwnPropertyDescriptor(proto, "scrollWidth"),
    client: Object.getOwnPropertyDescriptor(proto, "clientWidth"),
    rect: Object.getOwnPropertyDescriptor(Element.prototype, "getBoundingClientRect"),
  };
  Object.defineProperty(proto, "scrollWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains("step-group-words") ? shown(this) : 0;
    },
  });
  Object.defineProperty(proto, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains("step-group-words") ? Math.min(room, shown(this)) : 0;
    },
  });
  Object.defineProperty(proto, "getBoundingClientRect", {
    configurable: true,
    value(this: HTMLElement) {
      return { width: wide(this) } as DOMRect;
    },
  });
  let resized: (() => void) | null = null;
  const RO = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class {
    constructor(cb: ResizeObserverCallback) {
      resized = () => cb([{ contentRect: { width: room } } as ResizeObserverEntry], this as never);
    }
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;
  try {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    const parts = ["2 papers read.", "Searched once.", "1 paper changed.", "And 2 more."];
    const more = "2 of them went on in the background.";
    await act(async () =>
      root!.render(
        <StepGroup
          id="tool_call:1"
          kinds={["read", "search", "edit"]}
          line={parts.join(" ")}
          parts={parts}
          more={more}
          trouble="1 failed."
          head={(l) => <div className="meta">{l}</div>}
        >
          {null}
        </StepGroup>,
      ),
    );
    const line = host.querySelector<HTMLButtonElement>(".meta > button")!;
    const seen = () =>
      [...line.querySelectorAll(".step-part:not(.step-part-dropped)")].map((e) =>
        e.textContent?.trim(),
      );
    const dropped = () =>
      [...line.querySelectorAll(".step-part-dropped")].map((e) => e.textContent?.trim());
    // 300 px: the aside, the trouble and the "…" take 245, so one sentence fits; the rest drop from the end
    expect(seen()).toEqual(["2 papers read."]);
    expect(dropped()).toEqual(["Searched once.", "1 paper changed.", "And 2 more."]);
    expect(line.querySelector(".step-part-cut[data-cut]")?.getAttribute("aria-hidden")).toBe(
      "true",
    );
    expect(line.querySelector(".step-group-more")?.textContent).toBe(more);
    expect(line.querySelector(".step-group-trouble")?.textContent).toBe("1 failed.");
    // the "…" follows the sentences, so on screen it stands after the last kept one (the dropped are out of the flow),
    // before the aside
    const order = [...line.querySelectorAll(".step-part, .step-part-cut")].map((e) => e.className);
    expect(order.at(-1)).toBe("step-part-cut");
    // the whole summary is still the hover and, unseen, in the name
    const full = `${parts.join(" ")} ${more} 1 failed.`;
    expect(line.title).toBe(full);
    for (const p of parts) expect(line.textContent).toContain(p);
    // wider: measured again, everything fits, no "…"
    room = 700;
    await act(async () => resized?.());
    expect(seen()).toEqual(parts);
    expect(line.querySelector(".step-part-cut[data-cut]")).toBeNull();
    // very narrow: one sentence stays, cut mid-word by the CSS as a last resort
    room = 100;
    await act(async () => resized?.());
    expect(seen()).toEqual(["2 papers read."]);
  } finally {
    if (saved.scroll) Object.defineProperty(proto, "scrollWidth", saved.scroll);
    else delete (proto as { scrollWidth?: number }).scrollWidth;
    if (saved.client) Object.defineProperty(proto, "clientWidth", saved.client);
    else delete (proto as { clientWidth?: number }).clientWidth;
    delete (proto as { getBoundingClientRect?: unknown }).getBoundingClientRect;
    if (
      saved.rect &&
      Object.getOwnPropertyDescriptor(Element.prototype, "getBoundingClientRect") === undefined
    )
      Object.defineProperty(Element.prototype, "getBoundingClientRect", saved.rect);
    globalThis.ResizeObserver = RO;
  }
});
