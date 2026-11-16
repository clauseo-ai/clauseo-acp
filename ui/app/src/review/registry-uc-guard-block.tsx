// UC's entries for the steps Clauseo's pre-write guard stopped (UC-700 to UC-706): a host notice in the room's thread,
// in the app's words (room/thread/notice-words.ts), for a ChatGPT step that would change a paper open in Word or the
// matter's .codex folder. Codex shows no step of its own for a call its hook blocked; the notice is the lawyer's sign.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { uuid, userMessage } from "../testing/stream-script.ts";
import type { NoticeKind, ViewEntry } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { threadHost } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { threadWords } from "./words.ts";

const HELD: ThreadKey =
  "Clauseo stopped a step: {name} tried to change {file}, which is open in Word. Close it in Word, then ask again.";
const HELD_ALONE: ThreadKey =
  "Clauseo stopped a step: the AI tried to change {file}, which is open in Word. Close it in Word, then ask again.";
const CODEX: ThreadKey =
  "Clauseo stopped a step: {name} tried to change its own settings in this matter's folder.";
const CODEX_ALONE: ThreadKey =
  "Clauseo stopped a step: the AI tried to change its own settings in this matter's folder.";
const UNNAMED: ThreadKey = "Clauseo stopped a step.";
const ONE_MORE: ThreadKey = "Clauseo stopped one more step.";
const MORE: ThreadKey = "Clauseo stopped {n} more steps.";

/** The host's notice of `kind`, its English fallback as the host writes it. */
const notice = (kind: NoticeKind, text: string): ViewEntry => ({
  notice: { order: 6, event: uuid(1, 10), level: "warning", text, ts: 6000, kind },
});

/** The lawyer's question, then the notice, in a thread of codex-acp (ChatGPT). */
const thread =
  (entry: ViewEntry): (() => ReactNode) =>
  () => (
    <RoomOver
      host={() =>
        threadHost({
          agent: "codex-acp",
          entries: [userMessage(5, 1, "Add a clause on late payment to the lease."), entry],
        })
      }
    />
  );

/** A line where the room knows no agent name, which it never has today: rendered alone, as TN-12. */
const alone =
  (key: ThreadKey, slots?: Record<string, string>) =>
  (lang: Lang): ReactNode => (
    <main lang={lang} className="review-alone">
      <p>{th(lang, key, slots)}</p>
    </main>
  );

export const UC_GUARD_BLOCK: Entry[] = [
  {
    id: "UC-700",
    words: threadWords(HELD),
    screen: "room/thread",
    render: thread(
      notice(
        { held_open_blocked: { file: "Lease.docx" } },
        "Clauseo stopped a step: the AI tried to change Lease.docx, which is open in Word. Close it in Word, then ask again.",
      ),
    ),
  },
  {
    id: "UC-701",
    words: threadWords(HELD_ALONE),
    screen: "room/thread",
    render: alone(HELD_ALONE, { file: "Lease.docx" }),
  },
  {
    id: "UC-702",
    words: threadWords(CODEX),
    screen: "room/thread",
    render: thread(notice("codex_folder_blocked", CODEX_ALONE)),
  },
  {
    id: "UC-703",
    words: threadWords(CODEX_ALONE),
    screen: "room/thread",
    render: alone(CODEX_ALONE),
  },
  {
    id: "UC-704",
    words: threadWords(UNNAMED),
    screen: "room/thread",
    render: thread(notice("guard_step_blocked", UNNAMED)),
  },
  {
    id: "UC-705",
    words: threadWords(ONE_MORE),
    screen: "room/thread",
    render: thread(notice({ guard_steps_more: { more: 1 } }, ONE_MORE)),
  },
  {
    id: "UC-706",
    words: threadWords(MORE),
    screen: "room/thread",
    render: thread(notice({ guard_steps_more: { more: 3 } }, "Clauseo stopped 3 more steps.")),
  },
];
