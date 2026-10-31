// @vitest-environment happy-dom
// The room's title bar drags the window through its plan: the plan's meter carries the drag region on its four
// elements, and the plan's card, when it has one, is a press that does not drag.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";
import { RoomToolbar, type RoomToolbarProps } from "./room-toolbar.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

const toolbar: Omit<RoomToolbarProps, "plan"> = {
  title: "Acme Ltd v. State",
  titleHint: "Delhi High Court. CS 12/2026. Acme",
  nextDate: "3 March 2027",
  caseNo: "CS 12/2026",
  court: "Delhi High Court",
  history: [],
  canUndo: false,
  onUndo: () => {},
  railOpen: false,
  onRail: () => {},
  platform: "mac",
};

async function mount(plan: RoomToolbarProps["plan"]) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider className="room-window">
          <RoomToolbar {...toolbar} plan={plan} />
        </RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
}

/** How many of the element and its descendants carry the drag region. */
const dragging = (el: Element | null | undefined) =>
  el === null || el === undefined
    ? 0
    : [el, ...el.querySelectorAll("*")].filter((e) => e.hasAttribute("data-tauri-drag-region"))
        .length;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

test("the plan's meter in the title bar drags the window on its four elements", async () => {
  await mount({
    name: "Claude",
    left: 0.6,
    valueText: "60 percent of the plan left",
    status: "Plenty left",
  });
  const meter = host?.querySelector(".room-plan");
  expect(meter).not.toBeNull();
  expect(dragging(meter)).toBe(4);
});

test("a plan with a card keeps the drag region on the meter inside its press", async () => {
  await mount({
    name: "Claude",
    left: 0.5,
    valueText: "50 percent of the plan left",
    status: "Plenty left",
    cardLabel: "Show the plan's limits",
    card: <p>Every window</p>,
  });
  const press = host?.querySelector(".room-plan-press");
  expect(dragging(press)).toBe(4);
});
