// UC's entries for files deleted during a step that Clauseo kept no copy of (UC-720 to UC-725): the host's
// files_gone_no_copy notice in the room's thread (room/thread/notice-words.ts, from words-thread.ts goneLines), whose lines the band
// in Versions repeats for that change, and the timeline's short note on it. A file only online says its cloud
// service's recycle bin may still have it; a file too large says so.
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
  "{file} was deleted during this step. Clauseo had no copy of it to bring back. It was only in {service}, so {service}'s recycle bin may still have it.";
const ONLINE: ThreadKey =
  "{file} was deleted during this step. Clauseo had no copy of it to bring back. It was only online, so your cloud service's recycle bin may still have it.";
const LARGE: ThreadKey =
  "{file} was deleted during this step. Clauseo had no copy of it to bring back, because it was too large to keep versions of.";
const PLAIN: ThreadKey =
  "{file} was deleted during this step. Clauseo had no copy of it to bring back.";
const MORE: ThreadKey = "{n} more files that Clauseo had no copy of were deleted during this step.";
const NOTE: VersionKey = "{file} was deleted, with no copy to bring back";

/** The host's notice of the gone files, its English fallback as the host writes it. */
const notice = (files: SkippedFile[], more: number, text: string): ViewEntry => {
  const kind: NoticeKind = { files_gone_no_copy: { files, more } };
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
        `${BRIEF} was deleted during this step. Clauseo had no copy of it to bring back.`,
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
        `${SCAN} was deleted during this step. Clauseo had no copy of it to bring back.`,
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
        "7 files were deleted during this step that Clauseo had no copy of, so they can't be brought back.",
      ),
    ),
  },
  {
    id: "UC-725",
    words: versionWords(NOTE),
    screen: "room/versions",
    render: alone((lang) => vt(lang, NOTE, { file: "Site photos.zip" })),
  },
];
