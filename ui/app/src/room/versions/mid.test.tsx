// @vitest-environment happy-dom
// On a mid window (1000 to 1199 px) Versions' panel opens over the work, so picking a version puts it away and the
// band's Versions brings it back; wider, the panel stays; a window widened while it is away shows it again. The width
// comes from a media query's change events, never a timer.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN } from "../../testing/stream-script.ts";
import {
  answering,
  dialog,
  file,
  mountVersions,
  NOON,
  stepEntry,
  version,
  type VersionsHarness,
} from "./testing.ts";

/** A media query whose match the test sets, telling its listeners. */
function stubMedia(matches: boolean) {
  const listeners = new Set<() => void>();
  const query = {
    get matches() {
      return matches;
    },
    addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn),
  };
  vi.stubGlobal("matchMedia", () => query);
  return {
    set: async (next: boolean) => {
      matches = next;
      await act(async () => {
        for (const fn of listeners) fn();
      });
    },
  };
}

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

const VERSIONS = [
  version(1, "import"),
  version(2, "after_turn", { thread: THREAD, turn: TURN }),
  version(3, "outside_edits", { files: [file("other.txt")] }),
];
const STEPS = [stepEntry(5, { files: [file("brief.txt")], checkpoint_after: "cp2" })];
const lead = () => document.body.querySelector<HTMLElement>(".ver-band .ver-open-panel");

test("on a mid window, picking a version puts the panel away, and the band's Versions brings it back", async () => {
  stubMedia(true);
  h = await mountVersions(VERSIONS, STEPS);
  await h.openVersions();
  await h.lookAt("cp2");
  expect(h.one("versions")).toBeNull();
  // the band stands clear, with its acts and the way back into the panel
  expect(h.one("version-restore")).not.toBeNull();
  expect(lead()?.textContent).toBe("Versions3");
  expect(lead()?.getAttribute("aria-expanded")).toBe("false");
  await h.click(lead());
  expect(h.one("versions")).not.toBeNull();
  expect(lead()).toBeNull();
});

test("on a wide window the panel stays as a version is picked, and widening a mid window shows it again", async () => {
  const media = stubMedia(false);
  h = await mountVersions(VERSIONS, STEPS);
  await h.openVersions();
  await h.lookAt("cp2");
  expect(h.one("versions")).not.toBeNull();
  expect(lead()).toBeNull();
  await media.set(true);
  await h.lookAt("cp1");
  expect(h.one("versions")).toBeNull();
  await media.set(false);
  expect(h.one("versions")).not.toBeNull();
});

test("in the e2e's 1028 by 779 window, picking a version closes the panel and the band's Restore is reachable", async () => {
  stubMedia(true);
  h = await mountVersions(VERSIONS, STEPS, {
    query: answering({
      preview_restore: {
        restore_preview: { changes: [file("brief.txt")], added_since: [], kept: [], locked: [] },
      },
    }),
  });
  (
    globalThis as { happyDOM?: { setViewport: (v: { width: number; height: number }) => void } }
  ).happyDOM?.setViewport({ width: 1028, height: 779 });
  await h.openVersions();
  await h.lookAt("cp2");
  // nothing of the panel stands over the band: no Close of the panel, and Restore asks for its preview
  expect(h.one("versions")).toBeNull();
  expect(document.body.querySelector(".ver-panel")).toBeNull();
  await h.click(h.one("version-restore"));
  expect(h.queried("preview_restore")).toHaveLength(1);
  expect(dialog()).not.toBeNull();
});
