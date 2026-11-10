// A .docx's tracked changes in the wall's place (paper-wall.tsx opens it from the file's tile): the list read through
// revisions.ts, shown by the parts' WordChanges, with the lock's dialog (draft-review.tsx's WordLockDialog) when Word
// holds the file. Each change's time reads "Today, 3:42 pm" on the day it was made, else its day before its time, as
// the rail says a time ("Monday 9:00 am", "18 November 9:00 am"); the latest decision says its own time beside Undo.
import { useEffect, useState, useSyncExternalStore } from "react";
import { WordLockDialog } from "@clauseo/components/clauseo/draft-review.tsx";
import {
  WordChanges,
  type TrackedChange,
  type TrackedKind,
} from "@clauseo/components/clauseo/word-changes.tsx";
import { useDisplay, type Lang } from "@clauseo/components/lib/display.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import { baseName } from "@clauseo/components/lib/words-core.ts";
import { rdt } from "@clauseo/components/lib/words-reader.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { MatterId, Revision, Timestamp } from "../../transport/protocol.ts";
import { clockOf, localZone } from "../when.ts";
import { Revisions } from "./revisions.ts";

const KINDS: readonly TrackedKind[] = ["insert", "delete", "move", "format"];

/** When a change was made, in the language: "Today, 3:42 pm", else "Monday 3:42 pm"; "" where the file records no
 * time. */
function whenOf(at: Timestamp | undefined, now: Timestamp, zone: string, lang: Lang): string {
  if (at === undefined) return "";
  const { time, day } = clockOf(at, now, zone, lang);
  const said = clock(lang, time);
  return day === "" ? rdt(lang, "Today, {time}", { time: said }) : `${day} ${said}`;
}

const rowOf = (r: Revision, now: Timestamp, zone: string, lang: Lang): TrackedChange => ({
  key: r.ids.join(","),
  ids: r.ids,
  kind: KINDS.includes(r.kind) ? r.kind : "unknown",
  author: r.author,
  when: whenOf(r.at, now, zone, lang),
  text: r.text ?? null,
});

/** The tracked changes of `path`, a .docx of the matter, with Back to the wall. */
export function ChangesPane({
  transport,
  matter,
  path,
  onBack,
}: {
  transport: HostTransport;
  matter: MatterId;
  path: string;
  onBack: () => void;
}) {
  const { lang } = useDisplay();
  const [revisions] = useState(() => new Revisions(transport, matter, path));
  useEffect(() => revisions.start(), [revisions]);
  const s = useSyncExternalStore(revisions.subscribe, revisions.view, revisions.view);
  const zone = localZone();
  const file = baseName(path);
  const line =
    s.refused !== null
      ? { code: s.refused.code, retry: s.refused.again !== null }
      : s.unread !== null
        ? { code: s.unread, retry: false }
        : null;
  return (
    <>
      <WordChanges
        file={file}
        changes={s.list?.revisions.map((r) => rowOf(r, s.readAt, zone, lang)) ?? null}
        sending={s.sending}
        decided={
          s.last === null
            ? null
            : { decision: s.last.decision, time: clockOf(s.last.at, s.last.at, zone, "en").time }
        }
        line={line}
        onBack={onBack}
        onDecide={(ids, decision) => revisions.decide(ids, decision)}
        onUndo={() => void revisions.undo()}
        onRetry={() => revisions.again()}
      />
      <WordLockDialog
        file={file}
        open={s.locked !== null}
        act={s.lockAct}
        onClose={() => revisions.closeLock()}
        onTryAgain={() => revisions.again()}
      />
    </>
  );
}
