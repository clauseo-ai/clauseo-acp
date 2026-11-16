// @vitest-environment happy-dom
// The Smart App Control note in the real room over FakeTransport: it shows only while display_info says evaluation and
// "Got it" hasn't been pressed; "Got it" puts it away, and a remount with the kept flag shows nothing; "How to do this"
// opens the install guide's section through open_external; on, off and unknown show nothing.
import { afterEach, beforeEach, expect, test } from "vitest";
import { mountRoom, type Harness } from "../thread/harness.ts";
import type { SmartAppControl } from "../../transport/bindings.ts";
import { forgetSacNotedForTests } from "./kept.ts";
import { SAC_GUIDE } from "./guide.ts";

const KEY = "clauseo.smart-app-control-noted";

let h: Harness | null = null;
beforeEach(() => {
  localStorage.clear();
  forgetSacNotedForTests();
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

/** The room for a computer whose Smart App Control display_info reports as `state`. */
async function roomWith(state: SmartAppControl | null): Promise<Harness> {
  const room = await mountRoom(state === null ? {} : { smartAppControl: state });
  await room.settle();
  await room.settle();
  return room;
}

test("in evaluation, not put away before, the room shows the note with its words", async () => {
  h = await roomWith("evaluation");
  const note = h.one("sac-note");
  expect(note).not.toBeNull();
  expect(note?.textContent).toContain(
    "If Windows ever blocks Clauseo, turn off Smart App Control.",
  );
  expect(note?.querySelector("[role=status]")).not.toBeNull();
  expect(h.one("sac-note-how")?.textContent).toBe("How to do this");
  expect(h.one("sac-note-got-it")?.textContent).toBe("Got it");
});

test("Got it puts it away, keeps that, and a remount shows nothing", async () => {
  h = await roomWith("evaluation");
  await h.click(h.one("sac-note-got-it"));
  expect(h.one("sac-note")).toBeNull();
  expect(localStorage.getItem(KEY)).toBe("true");
  await h.unmount();
  document.body.replaceChildren();
  // a restart: this run's record gone, only storage left
  forgetSacNotedForTests();
  h = await roomWith("evaluation");
  expect(h.one("sac-note")).toBeNull();
  expect(h.transport.only("open_external")).toEqual([]);
});

test("How to do this opens the install guide's Smart App Control section through open_external", async () => {
  h = await roomWith("evaluation");
  await h.click(h.one("sac-note-how"));
  await h.settle();
  expect(h.transport.only("open_external")).toEqual([{ call: "open_external", url: SAC_GUIDE }]);
  expect(SAC_GUIDE).toBe(
    "https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/install/windows.md#smart-app-control",
  );
  // the link alone doesn't put it away
  expect(h.one("sac-note")).not.toBeNull();
});

test.each(["on", "off", null] as const)(
  "with Smart App Control %s the room shows no note",
  async (state) => {
    h = await roomWith(state);
    expect(h.one("sac-note")).toBeNull();
  },
);
