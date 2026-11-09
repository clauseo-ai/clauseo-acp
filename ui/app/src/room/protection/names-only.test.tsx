// @vitest-environment happy-dom
// #766's data_fence_names_only in the room: while the host says the data fence compares the data
// folder by name only, the open Not protected note adds one quiet line, gone once a view says
// false; the composer's line, the marks and the note's cause stay as the matter's protection says.
import { NAMES_ONLY, pw } from "@clauseo/components/lib/words-protection.ts";
import { tbt } from "@clauseo/components/lib/words-toolbar.ts";
import { act } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { MatterSummary, Protection, ProtectionView } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import { PROT, view } from "./fixtures.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.clear());
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const NAMES = { data_fence_names_only: true } as Partial<ProtectionView>;
const settle = async () => {
  for (let i = 0; i < 5; i += 1) await h!.settle();
};
const mount = async (protection: Protection, over: Partial<ProtectionView>) => {
  const m: MatterSummary = { ...THE_MATTER, protection };
  h = await mountRoom({
    matters: [m],
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    protection: { view: () => view(protection, over) },
  });
  await settle();
};
const line = () => h!.one("workspace-line");
const note = () => document.body.querySelector<HTMLElement>('[data-testid="np-note"]');
const openNote = async () => {
  if (note() === null) await h!.click(line());
  await settle();
};
const WORDS = pw("en", NAMES_ONLY);

test("the open note says the names-only line while the flag is true, and drops it on a view with false", async () => {
  await mount(PROT.refused, NAMES);
  await openNote();
  expect(note()?.textContent).toContain(WORDS);
  await act(async () =>
    h!.transport.protectionViewChanged(
      view(PROT.refused, { data_fence_names_only: false } as Partial<ProtectionView>),
    ),
  );
  await settle();
  expect(note()).not.toBeNull();
  expect(note()?.textContent).not.toContain(WORDS);
});

test("without the flag the note has no names-only line", async () => {
  await mount(PROT.needs, {});
  await openNote();
  expect(note()).not.toBeNull();
  expect(note()?.textContent).not.toContain(WORDS);
});

test.each([
  ["protected", PROT.b, null],
  ["not protected", PROT.refused, "set_up_refused"],
  ["switched off", PROT.off, "switched_off"],
] as const)(
  "%s: the flag changes no line, mark or cause in the room",
  async (_, protection, cause) => {
    localStorage.setItem(
      "clauseo.protection-explained",
      JSON.stringify({ computer: true, matters: [THE_MATTER.matter] }),
    );
    const seen = async (over: Partial<ProtectionView>) => {
      await mount(protection, over);
      const out = {
        cause: line()?.dataset["cause"] ?? null,
        line: h!.host.querySelector(".room-line")?.textContent ?? null,
        toolbar: h!.host.querySelector(".room-next .np-mark")?.textContent ?? null,
        rail: h!.host.querySelector(".room-matter-np") !== null,
        required: h!.one("protection-required") !== null,
      };
      await h!.unmount();
      h = null;
      document.body.replaceChildren();
      return out;
    };
    const before = await seen({});
    expect(before.cause).toBe(cause);
    expect(before.toolbar).toBe(cause === null ? null : tbt("en", "Not protected"));
    expect(await seen(NAMES)).toEqual(before);
    expect(document.body.textContent).not.toContain(WORDS);
  },
);
