// Where a registered string lives: its words file and English key, and its current text in either language, looked
// up from that file, so the page and the gate always compare against what the app shows.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorSentence, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { frame, type FrameKey } from "@clauseo/components/lib/words-frame.ts";
import { lim, type LimitKey } from "@clauseo/components/lib/words-limits.ts";
import { mf, type MatterFormKey } from "@clauseo/components/lib/words-matter-form.ts";
import { mt, type MatterKey } from "@clauseo/components/lib/words-matters.ts";
import { rt, type RoomKey } from "@clauseo/components/lib/words-room.ts";
import { brk, type BreakKey } from "@clauseo/components/lib/words-breaks.ts";
import { startup, type StartupKey } from "@clauseo/components/lib/words-startup.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { vt, type VersionKey } from "@clauseo/components/lib/words-versions.ts";
import { t, type Key } from "@clauseo/components/lib/words.ts";

export type WordsRef = {
  /** The words file, under ui/components/src/. */
  file: string;
  /** The English key in it. */
  key: string;
  /** The string's current text, its slots as written. */
  text: (lang: Lang) => string;
};

export const shared = (key: Key): WordsRef => ({
  file: "lib/words.ts",
  key,
  text: (lang) => t(lang, key),
});

export const frameWords = (key: FrameKey): WordsRef => ({
  file: "lib/words-frame.ts",
  key,
  text: (lang) => frame(lang, key),
});

export const roomWords = (key: RoomKey): WordsRef => ({
  file: "lib/words-room.ts",
  key,
  text: (lang) => rt(lang, key),
});

export const threadWords = (key: ThreadKey): WordsRef => ({
  file: "lib/words-thread.ts",
  key,
  text: (lang) => th(lang, key),
});

export const matterWords = (key: MatterKey): WordsRef => ({
  file: "lib/words-matters.ts",
  key,
  text: (lang) => mt(lang, key),
});

export const limitWords = (key: LimitKey): WordsRef => ({
  file: "lib/words-limits.ts",
  key,
  text: (lang) => lim(lang, key),
});

/** An error sentence of words-errors.ts. The one sentence it shares with words-thread.ts is that file's, and is
 * registered through threadWords. */
export const errorWords = (key: Exclude<ErrorKey, ThreadKey>): WordsRef => ({
  file: "lib/words-errors.ts",
  key,
  text: (lang) => errorSentence(lang, key),
});

export const matterFormWords = (key: MatterFormKey): WordsRef => ({
  file: "lib/words-matter-form.ts",
  key,
  text: (lang) => mf(lang, key),
});

export const versionWords = (key: VersionKey): WordsRef => ({
  file: "lib/words-versions.ts",
  key,
  text: (lang) => vt(lang, key),
});

export const startupWords = (key: StartupKey): WordsRef => ({
  file: "lib/words-startup.ts",
  key,
  text: (lang) => startup(lang, key),
});

export const breakWords = (key: BreakKey): WordsRef => ({
  file: "lib/words-breaks.ts",
  key,
  text: (lang) => brk(lang, key),
});
