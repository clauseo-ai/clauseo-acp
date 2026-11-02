// @vitest-environment happy-dom
// Who made each version, read from its kind as the timeline's rows say it; how a run of one conversation's versions
// folds behind its newest and the All / By Clauseo / By you switch; the row for a file too large to keep versions;
// and the row and band line for a change that couldn't be kept as a version.
import { clock } from "@clauseo/components/lib/words.ts";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, TURN, uuid } from "../../testing/stream-script.ts";
import type { CheckpointKind, ViewEntry } from "../../transport/protocol.ts";
import { clockOf, localZone } from "../when.ts";
import {
  file,
  mountVersions,
  NOON,
  stepEntry,
  thread,
  version,
  type VersionsHarness,
} from "./testing.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  // half past noon on the versions' day: every version below is today
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

const turn = (n: number) => uuid(n, 4);

const restored = (order: number, from: string, checkpoint: string): ViewEntry => ({
  marker: {
    order,
    event: uuid(order, 9),
    ts: NOON + order * 60_000,
    kind: { version_restored: { from, checkpoint, scope: "matter" } },
  },
});

const rowOf = (h: VersionsHarness, checkpoint: string): HTMLElement | undefined =>
  h.all("version").find((r) => r.dataset["checkpoint"] === checkpoint);
const whoText = (row: HTMLElement | undefined) =>
  row?.querySelector(".ver-entry-who")?.textContent ?? null;
const timeText = (row: HTMLElement | undefined) =>
  row?.querySelector(".ver-entry-time")?.textContent ?? null;

test("each kind of version says who made it, and a before_turn version shows only with files", async () => {
  h = await mountVersions(
    [
      version(1, "import"),
      version(2, "after_turn", { thread: THREAD, turn: turn(10) }),
      version(3, "outside_edits", { files: [file("notes.docx")] }),
      version(4, "undo", { thread: THREAD, turn: turn(10) }),
      version(5, "undo", { thread: THREAD, turn: turn(11) }),
      version(6, "restore"),
      restored(30, "cp3", "cp6"),
      version(7, "restore"),
      version(8, "some_new_kind" as CheckpointKind),
      version(9, "before_turn"),
      version(10, "before_turn", { files: [file("memo.docx")] }),
      version(11, "outside_edits", { files: [file("last.docx")] }),
    ],
    [],
    { threads: [thread(THREAD, "Draft reply")] },
  );
  await h.openVersions();
  const rows = h.all("version");
  expect(rows.map((r) => [r.dataset["checkpoint"], r.dataset["kind"]])).toEqual([
    ["cp11", "outside_edits"],
    ["cp10", "before_turn"],
    ["cp8", "some_new_kind"],
    ["cp7", "restore"],
    ["cp6", "restore"],
    ["cp5", "undo"],
    ["cp4", "undo"],
    ["cp3", "outside_edits"],
    ["cp2", "after_turn"],
    ["cp1", "import"],
  ]);
  expect(whoText(rowOf(h, "cp1"))).toBe("Imported 3 documents");
  expect(whoText(rowOf(h, "cp2"))).toBe("Clauseo, in Draft reply");
  expect(whoText(rowOf(h, "cp3"))).toBe("You, outside Clauseo");
  expect(rowOf(h, "cp3")?.textContent).toContain("notes.docx");
  // the undo names the after_turn version of its own turn, by that version's time
  const at2 = timeText(rowOf(h, "cp2"));
  expect(at2).toMatch(/\d/);
  expect(whoText(rowOf(h, "cp4"))).toBe(`Undo of the change at ${at2}`);
  expect(whoText(rowOf(h, "cp5"))).toBe("Undo of a change");
  // the restore names the version its marker restored from
  expect(whoText(rowOf(h, "cp6"))).toBe(`Restore to ${timeText(rowOf(h, "cp3"))} today`);
  expect(whoText(rowOf(h, "cp7"))).toBe("Restore of a version");
  // a kind this version doesn't know shows its time alone
  expect(whoText(rowOf(h, "cp8"))).toBe("");
  expect(timeText(rowOf(h, "cp8"))).toMatch(/\d/);
  // a before_turn version with files is the lawyer's, outside Clauseo; one without isn't shown
  expect(whoText(rowOf(h, "cp10"))).toBe("You, outside Clauseo");
  expect(rowOf(h, "cp10")?.textContent).toContain("memo.docx");
  expect(rowOf(h, "cp9")).toBeUndefined();
});

test("a run of one conversation's versions folds behind its newest, and the switch shows each hand's", async () => {
  h = await mountVersions(
    [
      version(1, "import"),
      version(2, "after_turn", { thread: THREAD, turn: turn(10) }),
      version(3, "after_turn", { thread: THREAD, turn: turn(11) }),
      version(4, "after_turn", { thread: THREAD, turn: turn(12) }),
      version(5, "after_turn", { thread: THREAD, turn: turn(13) }),
      version(6, "outside_edits", { files: [file("notes.docx")] }),
    ],
    [],
    { threads: [thread(THREAD, "Draft reply")] },
  );
  await h.openVersions();
  const shown = () => h?.all("version").map((r) => r.dataset["checkpoint"]);
  expect(shown()).toEqual(["cp6", "cp5", "cp1"]);
  const panel = h.one("versions");
  const fold = panel?.querySelector<HTMLButtonElement>(".ver-run-fold");
  expect(fold?.getAttribute("aria-expanded")).toBe("false");
  expect(fold?.textContent).toMatch(/^3 more · .+ to .+$/);
  await h.click(fold);
  expect(fold?.getAttribute("aria-expanded")).toBe("true");
  expect(shown()).toEqual(["cp6", "cp5", "cp4", "cp3", "cp2", "cp1"]);

  await h.click(h.button("By Clauseo", panel));
  expect(shown()).toEqual(["cp5", "cp4", "cp3", "cp2"]);
  await h.click(h.button("By you", panel));
  expect(shown()).toEqual(["cp6", "cp1"]);
  await h.click(h.button("All", panel));
  expect(shown()).toEqual(["cp6", "cp5", "cp4", "cp3", "cp2", "cp1"]);
});

test("a file too large to keep versions says so on its version's row", async () => {
  h = await mountVersions(
    [version(1, "import"), version(2, "after_turn", { thread: THREAD, turn: TURN })],
    [
      stepEntry(5, {
        files: [file("brief.txt")],
        skipped: [{ path: "bundle/exhibits.pdf", reason: "over_size_cap" }],
        checkpoint_after: "cp2",
      }),
    ],
  );
  await h.openVersions();
  expect(rowOf(h, "cp2")?.textContent).toContain("exhibits.pdf is too large to keep versions");
  expect(rowOf(h, "cp1")?.textContent).not.toContain("too large");
});

test("a turn's change that couldn't be kept as a version shows on the timeline and under the band", async () => {
  const ts = NOON + 20 * 60_000;
  h = await mountVersions(
    [version(1, "import")],
    [stepEntry(20, { files: [file("brief.txt")], ts })],
  );
  await h.openVersions();
  const row = h.one("versions")?.querySelector(".ver-unsaved");
  expect(row?.querySelector(".ver-unsaved-why")?.textContent).toBe("Not kept as a version.");
  expect(row?.querySelector(".ver-entry-who")?.textContent).toBe("Clauseo, in Draft reply");
  expect(row?.querySelector(".ver-entry-time")?.textContent).toBe(
    clock("en", clockOf(ts, Date.now(), localZone(), "en").time),
  );
  const notices = [...document.body.querySelectorAll(".ver-notice")].map((n) => n.textContent);
  expect(notices).toContain(
    "Clauseo changed brief.txt, and the change couldn't be kept as a version.",
  );
});
