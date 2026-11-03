// @vitest-environment happy-dom
// The status line's tool segment: after the plan, parted by the dot, and inside the line's fit, so a tool phrase that
// leaves no room wraps the line to two rather than cutting anything short, measured again when the phrase changes.
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { StatusLine, type ToolsSegment } from "./status-line.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: ReturnType<typeof createRoot> | null = null;
/** The line's words' width, as the engine would lay them out: a phrase longer than `room` overflows the line. */
let room = 1000;

const proto = HTMLElement.prototype;
const scroll = Object.getOwnPropertyDescriptor(proto, "scrollWidth");
const client = Object.getOwnPropertyDescriptor(proto, "clientWidth");
Object.defineProperty(proto, "scrollWidth", {
  configurable: true,
  get(this: HTMLElement) {
    if (!this.classList.contains("room-status-line")) return 0;
    // wrapped, the line takes two and nothing stands past it
    if (this.closest<HTMLElement>(".room-status")?.dataset["fit"] === "wrapped") return 0;
    return (this.textContent ?? "").length * 8;
  },
});
Object.defineProperty(proto, "clientWidth", {
  configurable: true,
  get(this: HTMLElement) {
    return this.classList.contains("room-status-line") ? room : 0;
  },
});

afterAll(() => {
  if (scroll !== undefined) Object.defineProperty(proto, "scrollWidth", scroll);
  if (client !== undefined) Object.defineProperty(proto, "clientWidth", client);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  room = 1000;
});

const segment = (summary: string): ToolsSegment => ({
  summary,
  node: <button type="button">{summary}</button>,
});

const wrap = (node: ReactNode) => <RoomScopeProvider>{node}</RoomScopeProvider>;

async function mount(node: ReactNode) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(wrap(node)));
}
const rerender = (node: ReactNode) => act(async () => root?.render(wrap(node)));

const status = () => host!.querySelector<HTMLElement>(".room-status");
const tools = () => host!.querySelector<HTMLElement>(".room-status-tools");

test("the tool segment follows the plan, parted by the dot, as the line's last part", async () => {
  await mount(
    <StatusLine
      took="Took 38 seconds"
      plan="Uses your Claude plan"
      hint={null}
      tools={segment("Web search on")}
    />,
  );
  const line = host!.querySelector(".room-status-line");
  expect(line?.lastElementChild).toBe(tools());
  expect(tools()?.firstElementChild?.className).toBe("room-dot");
  expect(tools()?.textContent).toBe("Web search on");
  expect(status()?.dataset["fit"]).toBe("full");
});

test("with nothing before it the segment stands alone, with no dot; with none there is no segment", async () => {
  await mount(<StatusLine took={null} plan="" hint={null} tools={segment("Browser off")} />);
  expect(tools()?.querySelector(".room-dot")).toBeNull();
  await rerender(<StatusLine took={null} plan="Uses your Claude plan" hint={null} />);
  expect(tools()).toBeNull();
});

test("a phrase that leaves no room wraps the line, cutting nothing, and the fit runs again as the phrase changes", async () => {
  room = 400;
  const plan = "Uses your Claude plan";
  await mount(<StatusLine took={null} plan={plan} hint={null} tools={segment("Browser on")} />);
  expect(status()?.dataset["fit"]).toBe("full");
  await rerender(
    <StatusLine
      took="Took 38 seconds"
      plan={plan}
      hint={null}
      tools={segment("Web search and browser on")}
    />,
  );
  expect(status()?.dataset["fit"]).toBe("wrapped");
  expect(tools()?.textContent).toBe("Web search and browser on");
  await rerender(<StatusLine took={null} plan={plan} hint={null} tools={segment("Browser on")} />);
  expect(status()?.dataset["fit"]).toBe("full");
});
