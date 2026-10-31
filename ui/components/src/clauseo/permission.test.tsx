// @vitest-environment happy-dom
// The prompt names the modifier key of the computer the display says it runs on: Cmd on the Mac, Ctrl on Windows and
// Linux. The OS comes from the display alone, never from the webview's navigator.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { DEFAULT_DISPLAY, showDisplay, type Platform } from "../lib/display.ts";
import { TooltipProvider } from "../ui/tooltip.tsx";
import { PermissionPrompt } from "./permission.tsx";
import { RoomScopeProvider } from "./room-scope-provider.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | undefined;
let root: ReturnType<typeof createRoot> | undefined;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  showDisplay(DEFAULT_DISPLAY);
});

async function alwaysButton(platform: Platform) {
  showDisplay({ ...DEFAULT_DISPLAY, motion: "reduced", platform });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root?.render(
      <TooltipProvider>
        <RoomScopeProvider>
          <PermissionPrompt paper="Notice to the bank" state="pending" onAnswer={() => {}} keys />
        </RoomScopeProvider>
      </TooltipProvider>,
    ),
  );
  const button = host.querySelectorAll("button")[1];
  if (button === undefined) throw new Error("no second answer");
  return {
    shortcut: button.getAttribute("aria-keyshortcuts"),
    kbd: button.querySelector(".perm-key")?.textContent,
  };
}

test.each([
  ["mac", "Meta+Enter", "⌘ Enter"],
  ["windows", "Control+Enter", "Ctrl Enter"],
  ["linux", "Control+Enter", "Ctrl Enter"],
] as const)("on %s, allowing for the matter is %s", async (platform, shortcut, kbd) => {
  expect(await alwaysButton(platform)).toEqual({ shortcut, kbd });
});
