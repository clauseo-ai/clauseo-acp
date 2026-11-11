// @vitest-environment happy-dom
// The browser row's Try again over FakeTransport, when Clauseo couldn't check the firm's browser policy: it sends
// recheck_browser_policy once and waits, disabled and busy, until the host's `browser_policy` app event ends the read;
// then the row follows what it found. The firm's block gets no Try again.
import { act } from "react";
import { settled } from "@clauseo/components/lib/late.ts";
import { afterEach, expect, test } from "vitest";
import { header, reset } from "../../testing/stream-script.ts";
import type { BrowserPolicyState, ToolState } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.innerHTML = "";
});

const browserOff = (reason: string): ToolState => ({
  tool: "browser",
  availability: { unavailable: { reason } } as ToolState["availability"],
});

/** The room with the app feed, the thread's header saying the browser is off for `reason`, its popover open. */
async function open(reason: string): Promise<Harness> {
  const harness = await mountRoom({
    next: { parts: ["app"], query: () => Promise.reject(new Error("not scripted")) },
  });
  const head = header();
  head.header!.tools = [browserOff(reason)];
  await harness.deliverAwaited([reset([], head)]);
  await act(() => settled());
  await harness.click(harness.one("tool-line"));
  await act(() => settled());
  return harness;
}

const inRow = (id: string) =>
  document.body.querySelector<HTMLElement>(
    `[data-testid="tool-row"][data-tool="browser"] [data-testid="${id}"]`,
  );
const rechecks = () =>
  h!.transport.only("dispatch").filter((c) => "recheck_browser_policy" in c.command.command);
const play = async (state: BrowserPolicyState) => {
  await act(async () => h!.transport.appEvent({ browser_policy: { state } }));
};

test("Try again sends recheck_browser_policy once and waits until the read ends; a clear read says ready", async () => {
  h = await open("browser_policy_unreadable");
  const button = inRow("tool-recheck") as HTMLButtonElement;
  expect(button.textContent).toBe("Try again");
  expect(button.disabled).toBe(false);
  await h.click(button);
  expect(rechecks().map((c) => c.command.command)).toEqual([{ recheck_browser_policy: {} }]);
  const waiting = inRow("tool-recheck") as HTMLButtonElement;
  expect(waiting.disabled).toBe(true);
  expect(waiting.getAttribute("aria-busy")).toBe("true");
  waiting.click();
  expect(rechecks()).toHaveLength(1);
  await play("checking");
  expect((inRow("tool-recheck") as HTMLButtonElement).disabled).toBe(true);
  await play("unreadable");
  // still not read: the row is as it was, and Try again can be pressed again
  expect((inRow("tool-recheck") as HTMLButtonElement).disabled).toBe(false);
  expect(inRow("tool-row-reason")?.textContent).toBe("Couldn't check browser settings");
  await play("clear");
  expect(inRow("tool-recheck")).toBeNull();
  expect(inRow("tool-row-reason")?.textContent).toBe("Ready for new conversations");
  expect(inRow("tool-row-full")).toBeNull();
});

test("the firm's block offers no Try again", async () => {
  h = await open("browser_blocked_by_policy");
  expect(inRow("tool-row-reason")?.textContent).toBe("Blocked by a browser policy");
  expect(inRow("tool-recheck")).toBeNull();
});
