// @vitest-environment happy-dom
// Two rooms on one page, under StrictMode, each with a permission prompt waiting. A key pressed in one room's composer
// answers that room's prompt and never the other's, whichever prompt listened first. The rooms' windows are the
// provider's own, as on a screen: nothing here registers them.
import { act, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, expect, test, vi } from "vitest";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { PermissionPrompt } from "./permission.tsx";
import type { PermissionAnswer } from "./permission-keys.ts";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function Room({ onAnswer }: { onAnswer: (a: PermissionAnswer) => void }) {
  return (
    <RoomScopeProvider>
      <PermissionPrompt paper="Notice to the bank" state="pending" onAnswer={onAnswer} keys />
      <button type="button" className="other">
        Other
      </button>
      <form data-composer="">
        <textarea />
      </form>
    </RoomScopeProvider>
  );
}

const answers = {
  a: vi.fn<(a: PermissionAnswer) => void>(),
  b: vi.fn<(a: PermissionAnswer) => void>(),
};
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

const inRoom = (name: "a" | "b", selector: string) => {
  const n = document.querySelector(`[data-test="${name}"] ${selector}`);
  if (n === null) throw new Error(`no ${selector} in room ${name}`);
  return n;
};
const key = (on: Element, k: string) =>
  act(async () => {
    on.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  });

beforeAll(async () => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(
      <StrictMode>
        <TooltipProvider>
          <div data-test="a">
            <Room onAnswer={answers.a} />
          </div>
          <div data-test="b">
            <Room onAnswer={answers.b} />
          </div>
        </TooltipProvider>
      </StrictMode>,
    ),
  );
});

afterAll(async () => {
  await act(async () => root.unmount());
  host.remove();
});

test("Enter in one room's composer answers only that room's prompt, in either room", async () => {
  expect(document.querySelectorAll(".perm-done")).toHaveLength(0);
  await key(inRoom("a", "textarea"), "Enter");
  expect(answers.a.mock.calls).toEqual([["once"]]);
  expect(answers.b).not.toHaveBeenCalled();
  // the second room too, so the order the prompts listened in can't decide it
  await key(inRoom("b", "textarea"), "Escape");
  expect(answers.b.mock.calls).toEqual([["deny"]]);
  expect(answers.a).toHaveBeenCalledTimes(1);
});

test("Enter on a focused button in a room keeps its own Enter, and answers no prompt", async () => {
  answers.a.mockClear();
  answers.b.mockClear();
  await key(inRoom("a", ".other"), "Enter");
  expect(answers.a).not.toHaveBeenCalled();
  expect(answers.b).not.toHaveBeenCalled();
});
