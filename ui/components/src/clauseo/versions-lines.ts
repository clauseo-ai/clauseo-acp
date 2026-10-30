// What Versions says around the timeline: the restore dialog's words for a restore or an undo, the lines under the
// band after one, while an answer runs, and over a sheet. Pure, composed from the model and the words of Versions.
import type { Lang } from "../lib/display.ts";
import { clock } from "../lib/words.ts";
import { listOf, vt } from "../lib/words-versions.ts";
import {
  addedSince,
  changedSince,
  dateOf,
  differing,
  isToday,
  timeOf,
  undoChange,
  whenOf,
  type Calendar,
  type NameOf,
  type Version,
} from "./versions-model.ts";

const when = (lang: Lang, cal: Calendar, v: Version) => ({
  time: timeOf(lang, v),
  date: dateOf(lang, cal, v),
});

/** The restore dialog's words for restoring the matter to a version: what it asks, what changes, what stays. */
export function restoreWords(
  lang: Lang,
  cal: Calendar,
  history: readonly Version[],
  toId: string,
  name: NameOf,
) {
  const to = history.find((v) => v.id === toId);
  const now = history[history.length - 1];
  if (to === undefined || now === undefined) return null;
  const changed = differing(to.state, now.state);
  const added = addedSince(to.state, now.state);
  const papers = listOf(lang, changed.map(name));
  return {
    title: isToday(cal, to)
      ? vt(lang, "Restore the matter to how it was at {time} today?", { time: timeOf(lang, to) })
      : vt(lang, "Restore the matter to how it was at {time} on {date}?", when(lang, cal, to)),
    body:
      changed.length === 0
        ? vt(lang, "Nothing will change. The matter already stands as it did then.")
        : changed.length === 1
          ? vt(lang, "1 document will change: {papers}. You can undo this.", { papers })
          : vt(lang, "{n} documents will change: {papers}. You can undo this.", {
              n: changed.length,
              papers,
            }),
    stays:
      added.length === 0
        ? null
        : vt(lang, "'{papers}', added since, stays.", { papers: listOf(lang, added.map(name)) }),
  };
}

/** The restore dialog's words for undoing one version's change, with the documents kept as they are now: what it
 * asks, what goes back, and each document changed again since, by whom and when. */
export function undoWords(
  lang: Lang,
  cal: Calendar,
  history: readonly Version[],
  id: string,
  kept: readonly string[],
  name: NameOf,
) {
  const v = history.find((x) => x.id === id);
  if (v === undefined) return null;
  const since = changedSince(history, id);
  const back = undoChange(history, id, kept, { day: cal.today, at: v.at })?.changed ?? [];
  const papers = listOf(lang, back.map(name));
  const time = whenOf(lang, cal, v);
  return {
    title:
      v.who.kind === "outside"
        ? vt(lang, "Undo the change you made outside Clauseo at {time}?", { time })
        : vt(lang, "Undo the change Clauseo made at {time}?", { time }),
    body:
      back.length === 0
        ? vt(lang, "Nothing goes back. Every document it changed is kept as it is now.")
        : back.length === 1
          ? vt(
              lang,
              "1 document goes back to how it was just before it: {papers}. The undo is kept as a version of its own.",
              { papers },
            )
          : vt(
              lang,
              "{n} documents go back to how they were just before it: {papers}. The undo is kept as a version of its own.",
              { n: back.length, papers },
            ),
    editedHead:
      since.length === 0
        ? null
        : since.length === 1
          ? vt(lang, "1 document changed again after it.")
          : vt(lang, "{n} documents changed again after it.", { n: since.length }),
    edited: since.map((e) => ({
      id: e.id,
      title: name(e.id),
      at:
        e.by.who.kind === "outside"
          ? vt(lang, "Changed by you, outside Clauseo, at {time}", { time: timeOf(lang, e.by) })
          : vt(lang, "Changed by Clauseo at {time}", { time: timeOf(lang, e.by) }),
    })),
  };
}

/** The line under the bar after a restore: "Restored to 3:42 pm on 14 October. 3 documents changed." */
export function restoredLine(lang: Lang, cal: Calendar, to: Version, n: number): string {
  if (isToday(cal, to)) {
    const time = timeOf(lang, to);
    return n === 1
      ? vt(lang, "Restored to {time} today. 1 document changed.", { time })
      : vt(lang, "Restored to {time} today. {n} documents changed.", { time, n });
  }
  return n === 1
    ? vt(lang, "Restored to {time} on {date}. 1 document changed.", when(lang, cal, to))
    : vt(lang, "Restored to {time} on {date}. {n} documents changed.", {
        ...when(lang, cal, to),
        n,
      });
}

/** The line after undoing a restore: the matter stands as it did at the time of the version before it. */
export const undidRestoreLine = (lang: Lang, at: string) =>
  vt(lang, "Undid the restore. The matter stands as it did at {time}.", { time: clock(lang, at) });

/** The line after undoing a change: "Undid the change at 4:31 pm. Draft reply went back, ..." */
export const undidChangeLine = (lang: Lang, of: string, changed: readonly string[], name: NameOf) =>
  vt(
    lang,
    "Undid the change at {time}. {papers} went back, and the undo is a version of its own.",
    {
      time: clock(lang, of),
      papers: listOf(lang, changed.map(name)),
    },
  );

/** While an answer runs: "Restore and Undo wait until the answer in Draft is done, expected by 5:16 pm." */
export const waitingLine = (lang: Lang, conversation: string, due: string) =>
  vt(
    lang,
    "Restore and Undo wait until the answer in {conversation} is done, expected by {time}.",
    {
      conversation,
      time: clock(lang, due),
    },
  );

/** The band's note on a version that added a file too large to keep versions of; cap: the largest file kept. */
export const largeNote = (lang: Lang, v: Version, cap: string): string | null =>
  v.large === undefined
    ? null
    : vt(
        lang,
        "{file} is {size}, too large to keep versions. Versions keep files up to {cap}. Clauseo can read it but not change it, and Restore and Undo leave it as it is.",
        {
          file: v.large.file,
          size: v.large.size,
          cap,
        },
      );

/** Over the sheet: "Draft reply, as it stood at 3:42 pm on 14 October", or as it stands now. */
export function sheetLabel(lang: Lang, cal: Calendar, paper: string, v: Version | null): string {
  if (v === null) return vt(lang, "{paper}, as it stands now", { paper });
  return isToday(cal, v)
    ? vt(lang, "{paper}, as it stood at {time} today", { paper, time: timeOf(lang, v) })
    : vt(lang, "{paper}, as it stood at {time} on {date}", { paper, ...when(lang, cal, v) });
}
