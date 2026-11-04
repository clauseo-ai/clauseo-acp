// A search hit carries no archived flag: Find reads it from every matter's thread list (all-threads.ts), so an
// archived conversation in a matter the lawyer never opened still says "Archived"; the open matter's own list, which
// leaves archived threads out, stands in only where the app feed hasn't given a matter's list.
import { expect, test } from "vitest";
import type { MatterId, ThreadId, ThreadSummary } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import type { AllThreadsView } from "./all-threads.ts";
import type { ConversationsView } from "./conversations-view.ts";
import { archivedIn } from "./find.ts";

const OPEN: MatterId = uuid(1, 2);
const OTHER: MatterId = uuid(2, 2);
const UNFED: MatterId = uuid(3, 2);
const T1: ThreadId = uuid(1, 1);
const T2: ThreadId = uuid(2, 1);
const T3: ThreadId = uuid(3, 1);

const thread = (
  matter: MatterId,
  id: ThreadId,
  status: ThreadSummary["status"],
): ThreadSummary => ({
  thread: id,
  matter,
  agent: "fake-agent",
  status,
});

const every: AllThreadsView = {
  lists: new Map([[OTHER, [thread(OTHER, T1, "archived"), thread(OTHER, T2, "idle")]]]),
  now: 0,
};
const talks = {
  matter: UNFED,
  phase: "ready",
  rows: [{ key: T1, thread: T1 }],
} as unknown as ConversationsView;

test("an archived thread in a matter never opened says so; a live one doesn't", () => {
  const archived = archivedIn(every, talks);
  expect(archived(OTHER, T1)).toBe(true);
  expect(archived(OTHER, T2)).toBe(false);
});

test("without the matter's list, the open matter's list stands in: a thread it leaves out is archived", () => {
  const archived = archivedIn(every, talks);
  expect(archived(UNFED, T3)).toBe(true);
  expect(archived(UNFED, T1)).toBe(false);
  // a matter neither fed nor open: not known to be archived
  expect(archived(OPEN, T3)).toBe(false);
});
