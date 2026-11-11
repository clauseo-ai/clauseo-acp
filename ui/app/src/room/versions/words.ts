// What Versions says in the room, composed from the host's answers and the words of Versions (words-versions.ts):
// the undo and restore dialogs, the lines under the band after an act, and the line that says Restore and Undo wait.
// Pure: the language, the calendar and the paper names come in as arguments.
import type { RestoreAsk } from "@clauseo/components/clauseo/restore-dialog.tsx";
import type { BandNotice } from "@clauseo/components/clauseo/version-band.tsx";
import {
  timeOf,
  whenOf,
  type Calendar,
  type Version,
} from "@clauseo/components/clauseo/versions-model.ts";
import type { Lang } from "@clauseo/components/lib/display.ts";
import { clock } from "@clauseo/components/lib/words.ts";
import { errorWords } from "@clauseo/components/lib/words-errors.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { listOf, vt } from "@clauseo/components/lib/words-versions.ts";
import type {
  FolderPlace,
  SkippedFileReason,
  ThreadId,
  Timestamp,
} from "../../transport/protocol.ts";
import { serviceName } from "../sync-services.ts";
import { clockOf } from "../when.ts";
import { changedSince, type Ask, type Done, type Snapshot, type Unsaved } from "./state.ts";
import { fileName, namesOf } from "@clauseo/components/lib/words-core.ts";

/** A conversation's name as the rail gives it (conversations.ts's titleOf); one without a title is a new
 * conversation. */
export const conversationOf = (lang: Lang, s: Snapshot, thread: ThreadId): string =>
  s.titles.get(thread) || mt(lang, "New conversation");

/** The versions as the parts show them: one made in a conversation without a title names a new conversation, as
 * conversationOf does. */
export const named = (lang: Lang, history: readonly Version[]): readonly Version[] =>
  history.some((v) => v.who.kind === "clauseo" && v.who.conversation === "")
    ? history.map((v) =>
        v.who.kind === "clauseo" && v.who.conversation === ""
          ? { ...v, who: { ...v.who, conversation: mt(lang, "New conversation") } }
          : v,
      )
    : history;

/** A moment's clock time in English, "4:31 pm", for the words' {time} slots, which put it in the language. */
const at = (s: Snapshot, ts: Timestamp) => clockOf(ts, s.now, s.zone, "en").time;

/** While an answer runs, or after a refusal said one does: the line that says Restore and Undo wait. Never a time:
 * the room doesn't estimate when an answer ends. */
export function waitingOf(lang: Lang, s: Snapshot): string | null {
  if (s.busy !== null)
    return vt(lang, "Restore and Undo wait until the answer in {conversation} is done.", {
      conversation: conversationOf(lang, s, s.busy),
    });
  return s.refused ? errorWords(lang, "matter.turn_running") : null;
}

/** "{when}" of a version by its id, for the dialogs' titles. */
const versionOf = (s: Snapshot, id: string): Version | undefined =>
  s.timeline.history.find((v) => v.id === id);

/** The dialog's words for what the controller asks. */
export function askOf(lang: Lang, cal: Calendar, s: Snapshot, ask: Ask | null): RestoreAsk | null {
  if (ask === null) return null;
  if (ask.stage === "word") return { kind: "word", act: ask.act, files: ask.files.map(fileName) };
  const v = versionOf(s, ask.id);
  if (v === undefined) return null;
  // a refusal that named no file: the dialog stays as it was, says why, and offers Try again
  const failed = ask.failed === null ? null : errorWords(lang, ask.failed);
  if (ask.act === "restore") {
    const { changes, added_since, kept } = ask.preview;
    // undoing a restore: the files it added go (an older host's answer names none)
    const removes = ask.undoes === null ? [] : (ask.preview.removes ?? []);
    const name = namesOf([...changes.map((c) => c.path), ...added_since, ...kept, ...removes]);
    const changed = changes.map((c) => name(c.path));
    const papers = listOf(lang, changed);
    // one name stands in the line's own quotes; several are each quoted, and "and" joins them outside the quotes
    const quoted = (paths: readonly string[]) =>
      listOf(
        lang,
        paths.map((p) => `'${name(p)}'`),
      );
    const removed =
      removes.length === 0
        ? null
        : removes.length === 1
          ? vt(lang, "'{papers}', added by the restore, will be removed.", {
              papers: name(removes[0] ?? ""),
            })
          : vt(lang, "{papers}, added by the restore, will be removed.", {
              papers: quoted(removes),
            });
    const stays = [
      changed.length === 0 ? null : removed,
      added_since.length === 0
        ? null
        : added_since.length === 1
          ? vt(lang, "'{papers}', added since, stays.", { papers: name(added_since[0] ?? "") })
          : vt(lang, "{papers}, added since, stay.", { papers: quoted(added_since) }),
      kept.length === 0
        ? null
        : kept.length === 1
          ? vt(lang, "'{papers}' can't be restored and stays as it is.", {
              papers: name(kept[0] ?? ""),
            })
          : vt(lang, "{papers} can't be restored and stay as they are.", {
              papers: quoted(kept),
            }),
    ].filter((x) => x !== null);
    // undoing a restore names the restore it takes back, by when it was made
    const undone = ask.undoes === null ? undefined : versionOf(s, ask.undoes);
    return {
      kind: "confirm",
      act: "restore",
      title:
        undone !== undefined
          ? vt(lang, "Undo the restore at {time}?", { time: whenOf(lang, cal, undone) })
          : v.day === cal.today
            ? vt(lang, "Restore the matter to how it was at {time} today?", {
                time: timeOf(lang, v),
              })
            : vt(lang, "Restore the matter to how it was at {time} on {date}?", {
                time: timeOf(lang, v),
                date: cal.words(lang, v.day, "date"),
              }),
      body:
        changed.length === 0
          ? removed === null
            ? vt(lang, "Nothing will change. The matter already stands as it did then.")
            : // the removals alone: the dialog still says the act can be undone
              `${removed} ${vt(lang, "You can undo this.")}`
          : changed.length === 1
            ? vt(lang, "1 document will change: {papers}. You can undo this.", { papers })
            : vt(lang, "{n} documents will change: {papers}. You can undo this.", {
                n: changed.length,
                papers,
              }),
      stays: stays.length === 0 ? null : stays.join(" "),
      edited: [],
      editedHead: null,
      failed,
      ...(undone === undefined ? {} : { confirm: vt(lang, "Undo the restore") }),
    };
  }
  const meta = s.timeline.meta.get(ask.id);
  const files = meta?.step?.files ?? [];
  const name = namesOf(files.map((f) => f.path));
  const back = files.filter((f) => ask.choices.get(f.path) === "undo").map((f) => name(f.path));
  const since = changedSince(s.timeline, ask.id);
  // edited: the host says the folder's copy differs from the version's now, though no newer version shows it
  const edited = ask.edited ?? new Set<string>();
  const later = (path: string) => since.has(path) || edited.has(path);
  // the rows: each file kept by default, a file changed again since saying by whom and when (or since when, where only
  // the host saw it), one that may hold the lawyer's own edits saying so
  const rows = files.filter((f) => f.may_include_lawyer_edits || later(f.path));
  const sinceOnly = rows.every((f) => later(f.path));
  const mayOnly = rows.every((f) => !later(f.path));
  const n = rows.length;
  const papers = listOf(lang, back);
  return {
    kind: "confirm",
    act: "undo",
    title:
      v.who.kind === "outside"
        ? vt(lang, "Undo the change you made outside Clauseo at {time}?", {
            time: whenOf(lang, cal, v),
          })
        : vt(lang, "Undo the change Clauseo made at {time}?", { time: whenOf(lang, cal, v) }),
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
    stays: null,
    editedHead:
      n === 0
        ? null
        : sinceOnly
          ? n === 1
            ? vt(lang, "1 document changed again after it.")
            : vt(lang, "{n} documents changed again after it.", { n })
          : mayOnly
            ? n === 1
              ? vt(lang, "1 document may hold your own edits.")
              : vt(lang, "{n} documents may hold your own edits.", { n })
            : vt(lang, "Choose for each document."),
    edited: rows.map((f) => {
      const by = since.get(f.path);
      const newer = by === undefined ? undefined : versionOf(s, by);
      return {
        id: f.path,
        title: name(f.path),
        at:
          newer === undefined
            ? edited.has(f.path)
              ? v.day === cal.today
                ? vt(lang, "Changed since {time} today", { time: timeOf(lang, v) })
                : vt(lang, "Changed since {time} on {date}", {
                    time: timeOf(lang, v),
                    date: cal.words(lang, v.day, "date"),
                  })
              : vt(lang, "Changed during the answer, though Clauseo didn't say it was changing it.")
            : newer.who.kind === "clauseo"
              ? vt(lang, "Changed by Clauseo at {time}", { time: timeOf(lang, newer) })
              : vt(lang, "Changed by you, outside Clauseo, at {time}", {
                  time: timeOf(lang, newer),
                }),
        choice: ask.choices.get(f.path) ?? "keep",
      };
    }),
    failed,
  };
}

/** The line under the band for what the room last did; without its Undo, which the band adds. */
export function doneLine(lang: Lang, cal: Calendar, s: Snapshot, done: Done): string {
  switch (done.kind) {
    case "restored": {
      const to = versionOf(s, done.to);
      const time = to === undefined ? "" : timeOf(lang, to);
      if (to === undefined || to.day === cal.today)
        return done.n === 1
          ? vt(lang, "Restored to {time} today. 1 document changed.", { time })
          : vt(lang, "Restored to {time} today. {n} documents changed.", { time, n: done.n });
      const date = cal.words(lang, to.day, "date");
      return done.n === 1
        ? vt(lang, "Restored to {time} on {date}. 1 document changed.", { time, date })
        : vt(lang, "Restored to {time} on {date}. {n} documents changed.", {
            time,
            date,
            n: done.n,
          });
    }
    case "undidRestore":
      return vt(lang, "Undid the restore. The matter stands as it did at {time}.", {
        time: timeOfTs(lang, s, done.at),
      });
    case "undidChange":
      return vt(
        lang,
        "Undid the change at {time}. {papers} went back, and the undo is a version of its own.",
        { time: timeOfTs(lang, s, done.of), papers: listOf(lang, done.papers.map(fileName)) },
      );
  }
}

/**  The lines under the band for what the room last did: doneLine's, then, after an undo, one for
 * each file it kept because Clauseo had no earlier copy (cantUndoWords). An undo that put none
 * back says only those: it has no paper to name as gone back. */
export function doneLines(
  lang: Lang,
  cal: Calendar,
  s: Snapshot,
  done: Done,
  place: FolderPlace | undefined,
): { text: string; kept: boolean }[] {
  const kept = done.kind === "undidChange" ? (done.kept ?? []) : [];
  const own =
    done.kind === "undidChange" && done.papers.length === 0 && kept.length > 0
      ? []
      : [{ text: doneLine(lang, cal, s, done), kept: false }];
  return [
    ...own,
    ...kept.map((f) => ({ text: cantUndoWords(lang, f.path, f.reason, place), kept: true })),
  ];
}

/**  Why an undo left a file as it is: Clauseo had no earlier copy of it. A file that was only in
 * the cloud, or in a folder Clauseo couldn't read when the folder syncs with a cloud service, says
 * where its earlier copy may be, by the folder's sync service where the room knows one
 * (sync-services.ts); any other (over the size cap, a folder not synced, a reason not known) says
 * only that Clauseo kept no copy. */
export function cantUndoWords(
  lang: Lang,
  path: string,
  reason: SkippedFileReason,
  place: FolderPlace | undefined,
): string {
  const file = fileName(path);
  const synced = typeof place === "object" && "synced" in place;
  if (reason !== "cloud_only" && !(reason === "unread_folder" && synced))
    return vt(
      lang,
      "Clauseo can't undo the change to {file}. It didn't keep a copy of it before, so there's no earlier copy to bring back.",
      { file },
    );
  const service = serviceName(place);
  return service === null
    ? vt(
        lang,
        "Clauseo can't undo the change to {file}. It was only online before, so there's no earlier copy here. Your cloud service's version history may have the earlier one.",
        { file },
      )
    : vt(
        lang,
        "Clauseo can't undo the change to {file}. It was only in {service} before, so there's no earlier copy here. {service}'s version history has the earlier one.",
        { file, service },
      );
}

/** A moment's clock time in the language: "4:31 pm", or "शाम 4:31". */
const timeOfTs = (lang: Lang, s: Snapshot, ts: Timestamp) => clock(lang, at(s, ts));

/** The line for a failed act, by its code: never the host's message. */
export const failedLine = (lang: Lang, code: string): BandNotice => ({
  tone: "failed",
  text: errorWords(lang, code),
});

/** Why a Word document's changes couldn't be marked up, by the code (words-errors.ts's sentence): the Word tool
 * missing says to reinstall; any other failure, the host's or the transport's, says only that it couldn't be done
 * (the sheet that says the file changed stands under it). */
export const markFailedLine = (lang: Lang, code: string): string =>
  errorWords(lang, code === "blackline.no_word_tool" ? code : "blackline.failed");

/** The not-saved line: which paper Clauseo changed and when, that the change couldn't be kept as a version, and that
 * the paper stands as Clauseo left it. A change on another day says its day. */
export function unsavedLine(lang: Lang, s: Snapshot, u: Unsaved): string {
  const { time, day } = clockOf(u.ts, s.now, s.zone, lang);
  const paper = fileName(u.paper);
  return day === ""
    ? vt(
        lang,
        "Clauseo changed {paper} at {time}, and the change couldn't be kept as a version. {paper} stands as Clauseo left it.",
        { paper, time: clock(lang, time) },
      )
    : vt(
        lang,
        "Clauseo changed {paper} at {time} on {day}, and the change couldn't be kept as a version. {paper} stands as Clauseo left it.",
        { paper, time: clock(lang, time), day },
      );
}

/** The not-saved row's time, from its step (H17), in English for the panel's clock, and its day when not today (in
 * the language). */
export const unsavedAt = (lang: Lang, s: Snapshot, u: Unsaved): { at: string; day: string } => ({
  at: at(s, u.ts),
  day: clockOf(u.ts, s.now, s.zone, lang).day,
});
