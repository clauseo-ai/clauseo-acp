// @vitest-environment happy-dom
// New conversation is now's: started while Versions is open, by Cmd+N or Ctrl+N or by the rail's New conversation (on
// a mid window, where the rail stands while Versions' band stays over the work), it ends Versions first, so the rail
// comes back, the toolbar's Versions is closed, and the draft conversation is open.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import { file, mountVersions, NOON, stepEntry, version, type VersionsHarness } from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

const VERSIONS = [version(1, "import"), version(2, "after_turn", { thread: THREAD, turn: TURN })];
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const rail = () => h!.host.querySelector("nav.room-rail");
const band = () => document.body.querySelector(".ver-band");
const pair = () => h!.host.querySelector(".room-pair button[aria-expanded]");
const newRow = () =>
  h!
    .all("conversation")
    .find((r) => r.querySelector(".room-conv-title")?.textContent === "New conversation");

/** A media query that matches, or not, for the window's width. */
function stubMedia(matches: boolean) {
  vi.stubGlobal("matchMedia", () => ({
    matches,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

/** Presses New conversation's key with the modifier given; returns whether it was prevented. */
async function press(mods: KeyboardEventInit): Promise<boolean> {
  const e = new KeyboardEvent("keydown", {
    key: "n",
    code: "KeyN",
    bubbles: true,
    cancelable: true,
    ...mods,
  });
  await act(async () => void document.body.dispatchEvent(e));
  await h!.settle();
  return e.defaultPrevented;
}

test("Cmd+N or Ctrl+N while Versions is open ends Versions, the rail back, and opens a new conversation", async () => {
  stubMedia(false);
  h = await mountVersions(VERSIONS, STEPS);
  // the key New conversation names, read off its title before Versions takes the rail's place
  const mac = h.one("new-conversation")?.title === "⌘N";
  await h.openVersions();
  expect(h.one("versions")).not.toBeNull();
  expect(rail()).toBeNull();
  expect(await press(mac ? { metaKey: true } : { ctrlKey: true })).toBe(true);
  expect(h.one("versions")).toBeNull();
  expect(band()).toBeNull();
  expect(pair()?.getAttribute("aria-expanded")).toBe("false");
  expect(rail()).not.toBeNull();
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
});

test("on a mid window, the rail's New conversation with Versions' band over the work ends Versions too", async () => {
  stubMedia(true);
  h = await mountVersions(VERSIONS, STEPS);
  await h.openVersions();
  await h.lookAt("cp2");
  // the panel put away, Versions stays on cp2 with its band; Matters raises the rail and leaves Versions be
  expect(h.one("versions")).toBeNull();
  expect(band()).not.toBeNull();
  await h.click(h.one("rail-toggle"));
  expect(band()).not.toBeNull();
  await h.click(h.one("new-conversation"));
  expect(band()).toBeNull();
  expect(h.one("version-restore")).toBeNull();
  expect(newRow()?.getAttribute("aria-current")).toBe("true");
});
