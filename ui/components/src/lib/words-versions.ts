// The words of Versions, the whole matter as it stood at a time, with Compare, Restore and Undo, and of the edge of
// the matter: asking before going beyond it, the computer's own dialogs, and Not protected. As in words.ts the
// English is the key, the exact string on screen, and a key with slots fills them in either language; {time} takes
// clock(), so Hindi puts the part of the day first. The Hindi uses the formal आप, the danda, Western digits and
// English names in Latin script. Nothing here says I or मैं, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay, type Lang } from "./display.ts";
import { listOf, wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-versions.hi.ts";

/** Every string of Versions, in English: the matter as it stood, and the edge of the matter. */
export type VersionKey = keyof typeof HI;

/** A string of Versions in the language, its slots filled. */
export const vt = wordsOf("versions");

export { listOf };

/** Word's file names, each in single quotes, joined as the language joins a list: 'Lease.docx' and 'Notice.docx'. */
export function wordFiles(lang: Lang, names: readonly string[]): string {
  return new Intl.ListFormat(lang, { type: "conjunction" }).format(names.map((n) => `'${n}'`));
}

/** Versions' words of the page's language, bound, with the room's and the shared interface words beside them. */
export function useVersionWords() {
  const { lang } = useDisplay();
  return {
    lang,
    v: (key: VersionKey, slots?: Slots) => vt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
    list: (items: readonly string[]) => listOf(lang, items),
    files: (names: readonly string[]) => wordFiles(lang, names),
  };
}

/** What the computer's own dialog says at each edge, for OsDialogPreview. name: the folder's last part, "Precedents";
 * place: a website, "example.org"; matter: the matter's name as filed. */
export type EdgeAsk =
  | { kind: "folder"; name: string }
  | { kind: "site"; place: string }
  | { kind: "off"; matter: string };

export function edgeDialogWords(lang: Lang, ask: EdgeAsk) {
  const v = (key: VersionKey, slots?: Slots) => vt(lang, key, slots);
  if (ask.kind === "off") {
    return {
      title: v("Switch off protection for {matter}?", { matter: ask.matter }),
      lines: [
        v(
          "If you switch it off, Clauseo can read and change any file on this computer while it works on this matter, without asking.",
        ),
        v(
          "If you keep it, Clauseo stays inside this matter's folder, and the scanned annexures stay unread.",
        ),
      ] as const,
      foot: v("You can switch it back on from the line under the question box."),
      remember: null,
      confirm: v("Switch off"),
      cancel: v("Keep protection"),
      primary: "cancel" as const,
    };
  }
  const lines =
    ask.kind === "folder"
      ? ([
          v(
            "If you allow it, Clauseo can read every file in {name} for this matter, and what it reads goes to Claude as your papers do.",
            { name: ask.name },
          ),
          v("If you don't, Clauseo keeps to this matter's folder and answers without {name}.", {
            name: ask.name,
          }),
        ] as const)
      : ([
          v(
            "If you allow it, Clauseo can read pages on {place} and send it searches for this matter.",
            { place: ask.place },
          ),
          v(
            "If you don't, Clauseo answers from your papers and Indian Kanoon, without this website.",
          ),
        ] as const);
  return {
    title:
      ask.kind === "folder"
        ? v("Allow Clauseo to read the folder '{name}'?", { name: ask.name })
        : v("Allow Clauseo to open {place}?", { place: ask.place }),
    lines,
    foot: null,
    remember: v("Remember for this matter"),
    confirm: v("Allow"),
    cancel: v("Don't allow"),
    primary: "confirm" as const,
  };
}
