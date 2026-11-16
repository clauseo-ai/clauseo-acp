// @vitest-environment happy-dom
// The Windows line's notice (UC-820) in the real room over FakeTransport: it shows on Windows once the room lists a
// second matter, never with one matter, never on the Mac, and never again once "Got it" was pressed (kept in
// localStorage with when); its English is the copy registry's.
import { pw, WINDOWS_LINE } from "@clauseo/components/lib/words-protection.ts";
import { afterEach, beforeEach, expect, test } from "vitest";
import { APPROVALS } from "../../review/approvals.ts";
import { UC_WINDOWS_LINE } from "../../review/registry-uc-windows-line.tsx";
import type { MatterSummary } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "../thread/harness.ts";
import { forgetWindowsLineSeenForTests } from "./kept.ts";

const KEY = "clauseo.windows-line-seen";
const SECOND: MatterSummary = {
  matter: "00000000-0000-4000-8000-0000000000a2",
  name: "Mehta arbitration",
  kind: "folder",
};

let h: Harness | null = null;
beforeEach(() => {
  localStorage.clear();
  forgetWindowsLineSeenForTests();
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

async function roomOn(platform: "windows" | "mac", matters: MatterSummary[]): Promise<Harness> {
  const room = await mountRoom({ platform, matters });
  await room.settle();
  await room.settle();
  return room;
}

test("on Windows with two matters the room shows the notice, with UC-820's words and Got it", async () => {
  h = await roomOn("windows", [THE_MATTER, SECOND]);
  const note = h.one("windows-line");
  expect(note).not.toBeNull();
  expect(note?.textContent).toContain(pw("en", WINDOWS_LINE));
  expect(note?.querySelector("[role=status]")).not.toBeNull();
  expect(h.one("windows-line-got-it")?.textContent).toBe("Got it");
});

test("on Windows with one matter, nothing", async () => {
  h = await roomOn("windows", [THE_MATTER]);
  expect(h.one("windows-line")).toBeNull();
});

test("on the Mac with two matters, nothing", async () => {
  h = await roomOn("mac", [THE_MATTER, SECOND]);
  expect(h.one("windows-line")).toBeNull();
});

test("Got it puts it away and records when, and a remount after that shows nothing", async () => {
  h = await roomOn("windows", [THE_MATTER, SECOND]);
  await h.click(h.one("windows-line-got-it"));
  expect(h.one("windows-line")).toBeNull();
  const at = localStorage.getItem(KEY);
  expect(at).not.toBeNull();
  expect(Number.isNaN(Date.parse(at ?? ""))).toBe(false);
  await h.unmount();
  document.body.replaceChildren();
  // a restart: this run's record gone, only storage left
  forgetWindowsLineSeenForTests();
  h = await roomOn("windows", [THE_MATTER, SECOND]);
  expect(h.one("windows-line")).toBeNull();
});

test("seen before (storage alone), nothing", async () => {
  localStorage.setItem(KEY, "2026-11-16T09:00:00.000Z");
  h = await roomOn("windows", [THE_MATTER, SECOND]);
  expect(h.one("windows-line")).toBeNull();
});

test("the English is the copy registry's UC-820, provisional with Hindi later", () => {
  const entry = UC_WINDOWS_LINE.find((e) => e.id === "UC-820");
  expect(entry?.words.text("en")).toBe(pw("en", WINDOWS_LINE));
  expect(entry?.words.later).toBe(true);
  const approval = (APPROVALS as Record<string, { en: string; status: string; hi: unknown }>)[
    "UC-820"
  ];
  expect(approval?.en).toBe(pw("en", WINDOWS_LINE));
  expect(approval?.status).toBe("provisional");
  expect(approval?.hi).toBeNull();
});
