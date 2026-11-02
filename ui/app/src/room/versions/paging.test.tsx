// @vitest-environment happy-dom
// The matter's older versions page in for the stream's epoch: a Reset to a new epoch drops the pages read before it
// and reads the new window's, so no version between the two windows goes missing.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { THREAD, uuid } from "../../testing/stream-script.ts";
import {
  answering,
  file,
  matterReset,
  MATTER_STREAM,
  mountVersions,
  NOON,
  version,
  type VersionsHarness,
} from "./testing.ts";
import { loadVersions } from "./use-versions.ts";

let h: VersionsHarness | null = null;
beforeEach(() => {
  vi.setSystemTime(NOON + 30 * 60_000);
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  vi.useRealTimers();
});

/** The n-th version, by the lawyer and by Clauseo in turn, so no run of one hand folds. */
const edit = (n: number) =>
  n % 2 === 1
    ? version(n, "outside_edits", { files: [file(`f${n}.txt`)] })
    : version(n, "after_turn", { thread: THREAD, turn: uuid(n, 4) });

test("after a Reset to a new epoch, the older versions are paged in again for the new window", async () => {
  let older = [edit(1), edit(2)];
  h = await mountVersions([], [], {
    query: answering({ page_entries: () => ({ entries: older }) }),
  });
  await h.click(h.host.querySelector(".room-pair button[aria-expanded]"));
  await act(async () => void (await loadVersions()));
  await h.settle();
  await h.deliver(MATTER_STREAM, [matterReset([edit(3), edit(4)], 3, true)]);
  await h.settle();
  const rows = () => h?.all("version").map((r) => r.dataset["checkpoint"]);
  expect(rows()).toEqual(["cp4", "cp3", "cp2", "cp1"]);
  // the host restarts the stream: its new window starts at cp5, and cp3 and cp4 are older now
  older = [edit(1), edit(2), edit(3), edit(4)];
  await h.deliver(MATTER_STREAM, [matterReset([edit(5), edit(6)], 3, true)], uuid(2, 3));
  await h.settle();
  expect(h.queried("page_entries")).toHaveLength(2);
  expect(rows()).toEqual(["cp6", "cp5", "cp4", "cp3", "cp2", "cp1"]);
});
