// UC's entries for files deleted during an answer that Clauseo kept no copy of (UC-720 to UC-725), for a step the
// pre-write guard stopped because it would delete or replace one (UC-726, UC-727), and for files changed during an
// answer with no copy of how they were before (UC-728 to UC-731): the host's files_gone_no_copy notice in the room's
// thread (room/thread/notice-words.ts, from words-thread.ts goneLines), whose lines the band in Versions repeats for
// that change, the timeline's short note on it, and the files_changed_no_copy notice (changedLines). A file only
// online says its cloud service may still have it (among its deleted files, or an earlier version); a file too large
// says so.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { vt, type VersionKey } from "@clauseo/components/lib/words-versions.ts";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { NoticeKind, SkippedFile, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords, versionWords } from "./words.ts";

const IN_SERVICE: ThreadKey =
  "{file} was deleted during this answer. Clauseo had no copy of it to bring back. It was kept only in {service}, so {service} may still have it among its deleted files.";
const ONLINE: ThreadKey =
  "{file} was deleted during this answer. Clauseo had no copy of it to bring back. It was kept only in your cloud service, which may still have it among its deleted files.";
const LARGE: ThreadKey =
  "{file} was deleted during this answer. It was too large for Clauseo to keep a copy of, so Clauseo can't bring it back.";
const PLAIN: ThreadKey =
  "{file} was deleted during this answer. Clauseo had no copy of it to bring back.";
const MORE: ThreadKey = "{n} more files Clauseo had no copy of were deleted during this answer.";
const NOTE: VersionKey = "{file} was deleted; Clauseo had no copy to bring back.";
const STOPPED: ThreadKey =
  "Clauseo stopped a step: {name} tried to delete or replace {file}. Clauseo has no copy of it, so Undo couldn't have brought it back.";
const STOPPED_ALONE: ThreadKey =
  "Clauseo stopped a step: the AI tried to delete or replace {file}. Clauseo has no copy of it, so Undo couldn't have brought it back.";
const CHANGED: ThreadKey =
  "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back.";
const CHANGED_IN_SERVICE: ThreadKey =
  "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back. {service} may still have an earlier version.";
const CHANGED_ONLINE: ThreadKey =
  "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back. Your cloud service may still have an earlier version.";
const CHANGED_MORE: ThreadKey =
  "{n} more files Clauseo had no earlier copy of were changed during this answer.";

/** The guard's notice of a step it stopped on a file with no copy, in a thread of codex-acp (ChatGPT): Codex shows
 * no step of its own for a call its hook blocked, and Claude, its own failed step, gets no notice. */
const stopped = (): ReactNode => (
  <RoomOver
    host={() =>
      threadHost({
        agent: "codex-acp",
        entries: [
          userMessage(5, 1, "Tidy up the scans folder."),
          {
            notice: {
              order: 6,
              event: uuid(1, 10),
              level: "warning",
              text: "Clauseo stopped a step: the AI tried to delete or replace Site photos.zip. Clauseo has no copy of it, so Undo couldn't have brought it back.",
              ts: 6000,
              kind: { no_copy_blocked: { file: "Site photos.zip" } },
            },
          },
        ],
      })
    }
  />
);

/** The host's notice of the gone files, its English fallback as the host writes it. */
const notice = (files: SkippedFile[], more: number, text: string): ViewEntry => {
  const kind: NoticeKind = { files_gone_no_copy: { files, more } };
  return { notice: { order: 6, event: uuid(1, 10), level: "warning", text, ts: 6000, kind } };
};

/** The host's notice of files changed with no earlier copy, its English fallback as the host writes it. */
const changedNotice = (files: SkippedFile[], more: number, text: string): ViewEntry => {
  const kind: NoticeKind = { files_changed_no_copy: { files, more } };
  return { notice: { order: 6, event: uuid(1, 10), level: "warning", text, ts: 6000, kind } };
};

/** The lawyer's question, then the notice, in a thread of Claude's, in a folder no named service syncs. */
const thread =
  (entry: ViewEntry): (() => ReactNode) =>
  () => (
    <RoomOver
      host={() =>
        threadHost({
          entries: [userMessage(5, 1, "Tidy up the scans folder."), entry],
        })
      }
    />
  );

/** A line drawn alone, with its slots filled. */
const alone =
  (line: (lang: Lang) => string) =>
  (lang: Lang): ReactNode => (
    <main lang={lang} className="review-alone">
      <p>{line(lang)}</p>
    </main>
  );

const SCAN = "Scans/Site photos.zip";
const BRIEF = "Brief to counsel.docx";

export const UC_NO_COPY: Entry[] = [
  {
    id: "UC-720",
    words: threadWords(IN_SERVICE),
    screen: "room/thread",
    render: alone((lang) => th(lang, IN_SERVICE, { file: BRIEF, service: "OneDrive" })),
  },
  {
    id: "UC-721",
    words: threadWords(ONLINE),
    screen: "room/thread",
    render: thread(
      notice(
        [{ path: BRIEF, reason: "cloud_only" }],
        0,
        `${BRIEF} was deleted during this answer. Clauseo had no copy of it to bring back.`,
      ),
    ),
  },
  {
    id: "UC-722",
    words: threadWords(LARGE),
    screen: "room/thread",
    render: thread(
      notice(
        [{ path: SCAN, reason: "over_size_cap" }],
        0,
        `${SCAN} was deleted during this answer. Clauseo had no copy of it to bring back.`,
      ),
    ),
  },
  {
    id: "UC-723",
    words: threadWords(PLAIN),
    screen: "room/thread",
    render: alone((lang) => th(lang, PLAIN, { file: BRIEF })),
  },
  {
    id: "UC-724",
    words: threadWords(MORE),
    screen: "room/thread",
    render: thread(
      notice(
        [0, 1, 2, 3, 4].map((n) => ({ path: `Scans/Reel ${n + 1}.mov`, reason: "over_size_cap" })),
        2,
        "7 files were deleted during this answer that Clauseo had no copy of, so they can't be brought back.",
      ),
    ),
  },
  {
    id: "UC-725",
    words: versionWords(NOTE),
    screen: "room/versions",
    render: alone((lang) => vt(lang, NOTE, { file: "Site photos.zip" })),
  },
  {
    id: "UC-726",
    words: threadWords(STOPPED),
    screen: "room/thread",
    render: stopped,
  },
  {
    id: "UC-727",
    words: threadWords(STOPPED_ALONE),
    screen: "room/thread",
    render: alone((lang) => th(lang, STOPPED_ALONE, { file: "Site photos.zip" })),
  },
  {
    id: "UC-728",
    words: threadWords(CHANGED),
    screen: "room/thread",
    render: thread(
      changedNotice(
        [{ path: SCAN, reason: "over_size_cap" }],
        0,
        `${SCAN} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back.`,
      ),
    ),
  },
  {
    id: "UC-729",
    words: threadWords(CHANGED_IN_SERVICE),
    screen: "room/thread",
    render: alone((lang) => th(lang, CHANGED_IN_SERVICE, { file: BRIEF, service: "OneDrive" })),
  },
  {
    id: "UC-730",
    words: threadWords(CHANGED_ONLINE),
    screen: "room/thread",
    render: thread(
      changedNotice(
        [{ path: BRIEF, reason: "cloud_only" }],
        0,
        `${BRIEF} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back.`,
      ),
    ),
  },
  {
    id: "UC-731",
    words: threadWords(CHANGED_MORE),
    screen: "room/thread",
    render: thread(
      changedNotice(
        [0, 1, 2, 3, 4].map((n) => ({ path: `Scans/Reel ${n + 1}.mov`, reason: "over_size_cap" })),
        2,
        "7 files Clauseo had no earlier copy of were changed during this answer, so Undo can't bring the earlier versions back.",
      ),
    ),
  },
];
