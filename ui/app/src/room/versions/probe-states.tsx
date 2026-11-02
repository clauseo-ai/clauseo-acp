// The Versions pages of the probe build (probe-main.tsx's ?versions=panel, band, undo, restore and wait): the matter's
// history as room/versions/testing.ts and review/registry-u8.tsx script it (its import, a turn's version, the lawyer's
// edits outside Clauseo, the same turn again and its undo, a restore, a turn whose big file isn't kept, a turn whose
// change wasn't kept), its one conversation's steps, and the answers to list_threads, preview_restore and
// compare_versions. The builders are written out here, not imported from testing.ts, which imports vitest. Only the
// probe build imports this file. Not shipped.
import { matterStreamOf } from "../matter-stream.ts";
import {
  batch,
  event,
  header,
  MATTER,
  reset,
  STREAM,
  THREAD,
  TURN,
  uuid,
} from "../../testing/stream-script.ts";
import type {
  CheckpointKind,
  FileChange,
  HostEvent,
  HostQuery,
  QueryResult,
  RestorePreview,
  StepEntry,
  ViewEntry,
} from "../../transport/protocol.ts";

/** Which Versions page the probe loads: Versions open, a past version selected, the undo or restore dialog open, or
 * the conversation running (its Restore and Undo waiting). */
export type VersionsPage = "panel" | "band" | "undo" | "restore" | "wait";

/** ?versions=… as the probe reads it; any other value is no Versions page. */
export function versionsPageOf(value: string | null): VersionsPage | null {
  return value === "panel" ||
    value === "band" ||
    value === "undo" ||
    value === "restore" ||
    value === "wait"
    ? value
    : null;
}

const MINUTE = 60_000;
const NOTICE = "Notice to the bank.docx";
const REPLY = "Reply to the bank.docx";
const LEASE = "Lease.docx";
const ANNEXURE = "Annexure 3.pdf";
const DRAFT = "Draft 2.docx";
/** The conversation's second turn, whose version is undone, and a turn whose change wasn't kept. */
const SECOND_TURN = uuid(3, 4);
const UNSAVED_TURN = uuid(2, 4);

/** A local time, `days` from today. */
const atLocal = (days: number, h: number, m: number) => {
  const d = new Date();
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days, h, m).getTime();
};
/** Today's moments, `n` minutes apart from forty minutes ago (from just past midnight, so they stay today). */
const today = (n: number) => Math.max(atLocal(0, 0, 1), Date.now() - 40 * MINUTE) + n * MINUTE;

const file = (
  path: string,
  may = false,
  change: FileChange["change"] = "modified",
): FileChange => ({
  path,
  change,
  size_bytes: 1200,
  may_include_lawyer_edits: may,
});

const version = (
  n: number,
  kind: CheckpointKind,
  ts: number,
  files: FileChange[] = [],
  turn?: string,
): ViewEntry => ({
  version: {
    order: n,
    checkpoint: `cp${n}`,
    kind,
    ts,
    files,
    ...(turn === undefined ? {} : { thread: THREAD, turn }),
  },
});

const stepEntry = (order: number, over: Partial<StepEntry>): ViewEntry => ({
  step: {
    order,
    turn: TURN,
    stop: "end_turn",
    files: [],
    skipped: [],
    undone: false,
    ts: atLocal(-3, 10, 30),
    ...over,
  },
});

/** The matter's versions, oldest first: the import and a turn three days ago, the lawyer's edits outside Clauseo this
 * morning, the turn of today whose big file isn't kept, its undo, the restore to the turn of three days ago (with its
 * marker), and the conversation's last turn, whose change wasn't kept. */
function matterEntries(): ViewEntry[] {
  return [
    version(1, "import", atLocal(-3, 10, 0)),
    version(2, "after_turn", atLocal(-3, 10, 30), [file(NOTICE, true), file(REPLY)], TURN),
    version(3, "outside_edits", today(0), [file(LEASE)]),
    version(4, "after_turn", today(10), [file(REPLY)], SECOND_TURN),
    version(5, "undo", today(20), [file(REPLY)], SECOND_TURN),
    version(6, "restore", today(25)),
    {
      marker: {
        order: 60,
        event: uuid(60, 9),
        ts: today(25),
        kind: { version_restored: { from: "cp2", checkpoint: "cp6", scope: "matter" } },
      },
    },
  ];
}

/** The conversation's steps: its turn three days ago (kept as cp2), the turn that made cp4 with a file too large to keep
 * (undone since), and the last turn, whose change wasn't kept. */
function steps(): ViewEntry[] {
  return [
    stepEntry(9, { files: [file(NOTICE, true), file(REPLY)], checkpoint_after: "cp2" }),
    stepEntry(19, {
      turn: SECOND_TURN,
      files: [file(REPLY)],
      skipped: [{ path: ANNEXURE, reason: "over_size_cap" }],
      checkpoint_after: "cp4",
      undone: true,
      ts: today(10),
    }),
    stepEntry(29, { turn: UNSAVED_TURN, files: [file(REPLY)], ts: today(30) }),
  ];
}

/** The restore dialog's preview: two changed files, one brought back, and a file added since that the restore leaves. */
const PREVIEW: RestorePreview = {
  changes: [file(NOTICE), file(LEASE, false, "added")],
  added_since: [DRAFT],
  kept: [],
  locked: [],
};

/** What the probe's host answers and sends for one Versions page. */
export type VersionsHost = {
  /** The answer to a host query the page asks, or undefined for the probe to refuse it. */
  query: (query: HostQuery) => QueryResult | undefined;
  /** The events a subscription to `stream` is sent, at once, or none for a stream the page doesn't have. */
  subscribe: (stream: string) => HostEvent[];
};

/** The host for one Versions page: the conversation runs on wait, and every other page has it idle. */
export function versionsHost(page: VersionsPage): VersionsHost {
  const status: "idle" | "running" = page === "wait" ? "running" : "idle";
  return {
    query: (query) => {
      if (typeof query !== "object") return undefined;
      if (query.list_threads !== undefined)
        return {
          threads: [
            { thread: THREAD, matter: MATTER, agent: "claude", status, title: "Draft reply" },
          ],
        };
      if (query.preview_restore !== undefined) return { restore_preview: PREVIEW };
      if (query.compare_versions !== undefined) return { file_changes: [file(NOTICE)] };
      return undefined;
    },
    subscribe: (stream) => {
      if (stream === matterStreamOf(MATTER))
        return [
          event(
            batch(
              1,
              [
                {
                  reset: {
                    view_window: {
                      header: {
                        matter_header: {
                          order: 0,
                          matter: MATTER,
                          name: "Sharma v. State of Delhi",
                          kind: "folder",
                          import: { files: 3, bytes: 1000, cloud_only: 0 },
                        },
                      },
                      entries: matterEntries(),
                      has_older: false,
                    },
                  },
                },
              ],
              undefined,
              stream,
            ),
          ),
        ];
      if (stream === STREAM)
        return [event(batch(1, [reset(steps(), header(status))], undefined, stream))];
      return [];
    },
  };
}
