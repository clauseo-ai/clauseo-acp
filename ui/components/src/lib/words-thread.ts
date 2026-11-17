// The words of the live thread: the working line, errors, the permission prompt, the change note, the composer and
// the empty states. As in words.ts the English is the key, the exact string on screen, and a key with slots fills
// them in either language.
// The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits,
// English legal names in Latin script. Clauseo never says मैं, and nothing says पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay, type Lang } from "./display.ts";
import { fileName, wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-thread.hi.ts";

/** Every new string of the live thread, in English. */
export type ThreadKey = keyof typeof HI;

/** A new string in the language, its slots filled. */
export const th = wordsOf("thread");

/** The thread's words of the page's language, bound, with the shared ones beside them:
 * const w = useThreadWords(); w.th("Allow once"), w.t("Undo"), w.ui("Undo anyway"). */
export function useThreadWords() {
  const { lang } = useDisplay();
  return {
    lang,
    th: (key: ThreadKey, slots?: Slots) => th(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}

// Files deleted during a step that Clauseo kept no copy of (only online, or over the size cap), so no undo brings them
// back: the host's files_gone_no_copy notice in the thread (app's room/thread/notice-words.ts) and the note on that
// change in Versions' band (versions/words.ts goneNote) say the same lines, from here, which the first load holds.

/** A file gone with no copy: its path in the matter and why Clauseo had not versioned it (SkippedFileReason). */
export type GoneFile = { path: string; reason: string };

/** The line for one gone file, named as Versions names files: a file that was only online says its cloud service may
 * still have it among its deleted files, by the service's name where the room knows one; a file too large says so; any other
 * reason (one this version doesn't know) says only that Clauseo had no copy. */
export function goneLine(lang: Lang, f: GoneFile, service: string | null): string {
  const file = fileName(f.path);
  if (f.reason === "cloud_only")
    return service === null
      ? th(
          lang,
          "{file} was deleted during this answer. Clauseo had no copy of it to bring back. It was kept only in your cloud service, which may still have it among its deleted files.",
          { file },
        )
      : th(
          lang,
          "{file} was deleted during this answer. Clauseo had no copy of it to bring back. It was kept only in {service}, so {service} may still have it among its deleted files.",
          { file, service },
        );
  if (f.reason === "over_size_cap")
    return th(
      lang,
      "{file} was deleted during this answer. It was too large for Clauseo to keep a copy of, so Clauseo can't bring it back.",
      { file },
    );
  return th(
    lang,
    "{file} was deleted during this answer. Clauseo had no copy of it to bring back.",
    {
      file,
    },
  );
}

/** Every gone file's line, then, when `more` were gone than are named, one line counting the rest; joined by a space,
 * as one paragraph. Empty when none. */
export function goneLines(
  lang: Lang,
  files: readonly GoneFile[],
  more: number,
  service: string | null,
): string {
  const lines = files.map((f) => goneLine(lang, f, service));
  if (more > 0)
    lines.push(
      th(lang, "{n} more files Clauseo had no copy of were deleted during this answer.", {
        n: more,
      }),
    );
  return lines.join(" ");
}

// Files Clauseo kept no copy of (only online, or over the size cap) that are still there with another size or
// modification time: changed during the answer with no copy of how they were before, so Undo can't bring the earlier
// version back: the host's files_changed_no_copy notice in the thread (app's room/thread/notice-words.ts). A file only
// downloaded is not one (the host tells none).

/** The line for one changed file, named as Versions names files: a file that was only online adds that its cloud
 * service may still have an earlier version, by the service's name where the room knows one. */
export function changedLine(lang: Lang, f: GoneFile, service: string | null): string {
  const file = fileName(f.path);
  if (f.reason === "cloud_only")
    return service === null
      ? th(
          lang,
          "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back. Your cloud service may still have an earlier version.",
          { file },
        )
      : th(
          lang,
          "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back. {service} may still have an earlier version.",
          { file, service },
        );
  return th(
    lang,
    "{file} was changed during this answer. Clauseo had no copy of how it was before, so Undo can't bring the earlier version back.",
    { file },
  );
}

/** Every changed file's line, then, when `more` were changed than are named, one line counting the rest (the host
 * never leaves one alone to count: `more` is 0 or at least 2); joined by a space, as one paragraph. Empty when none. */
export function changedLines(
  lang: Lang,
  files: readonly GoneFile[],
  more: number,
  service: string | null,
): string {
  const lines = files.map((f) => changedLine(lang, f, service));
  if (more > 0)
    lines.push(
      th(lang, "{n} more files Clauseo had no earlier copy of were changed during this answer.", {
        n: more,
      }),
    );
  return lines.join(" ");
}
