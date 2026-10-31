// @vitest-environment happy-dom
// The accepted answer's Undo is named by the words it shows (WCAG 2.5.3, label in name), whatever language the
// display is in: the row's group label, "This answer", says which answer it undoes.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterAll, beforeAll, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay } from "../lib/display.ts";
import { Accept } from "./accept.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const WORDS = {
  action: "Accept into the matter",
  pending: "Not yet accepted",
  done: "Accepted",
  undo: "Undo",
};

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

beforeAll(async () => {
  showDisplay({ ...DEFAULT_DISPLAY, lang: "hi", motion: "reduced" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(
      <RoomScopeProvider>
        <Accept state="accepted" words={WORDS} onAccept={() => {}} onUndo={() => {}} />
      </RoomScopeProvider>,
    ),
  );
});

afterAll(() => {
  act(() => root.unmount());
  host.remove();
});

test("Undo's name is the word it shows, inside a group that names the answer", () => {
  const undo = host.querySelector<HTMLButtonElement>(".accept-undo");
  expect(undo?.textContent).toBe("Undo");
  // a name of its own, in the display's language, would not contain the word on screen
  expect(undo?.hasAttribute("aria-label")).toBe(false);
  expect(undo?.closest('[role="group"]')?.getAttribute("aria-label")).toBe("यह उत्तर");
});
