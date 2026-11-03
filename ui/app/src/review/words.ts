// Where a registered string lives: its words file and English key, and its current text in either language, looked
// up from that file, so the page and the gate always compare against what the app shows.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { errorHindi, errorSentence, type ErrorKey } from "@clauseo/components/lib/words-errors.ts";
import { frame, type FrameKey } from "@clauseo/components/lib/words-frame.ts";
import { lim, type LimitKey } from "@clauseo/components/lib/words-limits.ts";
import { limThread, type LimitThreadKey } from "@clauseo/components/lib/words-limits-thread.ts";
import { pc, type PlanCardKey } from "@clauseo/components/lib/words-plan-card.ts";
import { mf, type MatterFormKey } from "@clauseo/components/lib/words-matter-form.ts";
import { mt, type MatterKey } from "@clauseo/components/lib/words-matters.ts";
import { roomHindi, rt, type RoomKey } from "@clauseo/components/lib/words-room.ts";
import { rdt, type ReaderKey } from "@clauseo/components/lib/words-reader.ts";
import { brk, type BreakKey } from "@clauseo/components/lib/words-breaks.ts";
import { st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { startup, type StartupKey } from "@clauseo/components/lib/words-startup.ts";
import { steps, type StepKey } from "@clauseo/components/lib/words-steps.ts";
import { stepLive, type StepLiveKey } from "@clauseo/components/lib/words-steps-live.ts";
import { th, type ThreadKey } from "@clauseo/components/lib/words-thread.ts";
import { ui, type UiKey } from "@clauseo/components/lib/words-ui.ts";
import { vt, type VersionKey } from "@clauseo/components/lib/words-versions.ts";
import { t, type Key } from "@clauseo/components/lib/words.ts";

export type WordsRef = {
  /** The words file, under ui/components/src/; from the repo root for words the webview doesn't draw (H11's). */
  file: string;
  /** The English key in it. */
  key: string;
  /** The string's current text, its slots as written. */
  text: (lang: Lang) => string;
  /** The words file holds null Hindi for it (hi: later): the Hindi interface shows the English, and the approval
   * covers the English alone. */
  later: boolean;
};

export const shared = (key: Key): WordsRef => ({
  file: "lib/words.ts",
  key,
  text: (lang) => t(lang, key),
  later: t.hi[key] === null,
});

export const frameWords = (key: FrameKey): WordsRef => ({
  file: "lib/words-frame.ts",
  key,
  text: (lang) => frame(lang, key),
  later: frame.hi[key] === null,
});

export const roomWords = (key: RoomKey): WordsRef => ({
  file: "lib/words-room.ts",
  key,
  text: (lang) => rt(lang, key),
  later: roomHindi[key] === null,
});

export const threadWords = (key: ThreadKey): WordsRef => ({
  file: "lib/words-thread.ts",
  key,
  text: (lang) => th(lang, key),
  later: th.hi[key] === null,
});

export const matterWords = (key: MatterKey): WordsRef => ({
  file: "lib/words-matters.ts",
  key,
  text: (lang) => mt(lang, key),
  later: mt.hi[key] === null,
});

export const limitWords = (key: LimitKey): WordsRef => ({
  file: "lib/words-limits.ts",
  key,
  text: (lang) => lim(lang, key),
  later: lim.hi[key] === null,
});

/** A string of the limit the open thread brings (words-limits-thread.ts): the stop, the composer's warning. */
export const limitThreadWords = (key: LimitThreadKey): WordsRef => ({
  file: "lib/words-limits-thread.ts",
  key,
  text: (lang) => limThread(lang, key),
  later: limThread.hi[key] === null,
});

/** A string of a settled step group (words-steps.ts). */
export const stepWords = (key: StepKey): WordsRef => ({
  file: "lib/words-steps.ts",
  key,
  text: (lang) => steps(lang, key),
});

/** A sentence of the step in hand (words-steps-live.ts). */
export const stepLiveWords = (key: StepLiveKey): WordsRef => ({
  file: "lib/words-steps-live.ts",
  key,
  text: (lang) => stepLive(lang, key),
});

/** A string of the plan's card (words-plan-card.ts). */
export const planCardWords = (key: PlanCardKey): WordsRef => ({
  file: "lib/words-plan-card.ts",
  key,
  text: (lang) => pc(lang, key),
  later: pc.hi[key] === null,
});

/** An error sentence of words-errors.ts. The one sentence it shares with words-thread.ts is that file's, and is
 * registered through threadWords. */
export const errorWords = (key: Exclude<ErrorKey, ThreadKey>): WordsRef => ({
  file: "lib/words-errors.ts",
  key,
  text: (lang) => errorSentence(lang, key),
  later: errorHindi[key] === null,
});

export const matterFormWords = (key: MatterFormKey): WordsRef => ({
  file: "lib/words-matter-form.ts",
  key,
  text: (lang) => mf(lang, key),
  later: mf.hi[key] === null,
});

export const uiWords = (key: UiKey): WordsRef => ({
  file: "lib/words-ui.ts",
  key,
  text: (lang) => ui(lang, key),
  later: ui.hi[key] === null,
});

/** A string of the judgment reader and Word's lines (words-reader.ts), such as Open in Word. */
export const readerWords = (key: ReaderKey): WordsRef => ({
  file: "lib/words-reader.ts",
  key,
  text: (lang) => rdt(lang, key),
  later: rdt.hi[key] === null,
});

export const versionWords = (key: VersionKey): WordsRef => ({
  file: "lib/words-versions.ts",
  key,
  text: (lang) => vt(lang, key),
  later: vt.hi[key] === null,
});

export const startupWords = (key: StartupKey): WordsRef => ({
  file: "lib/words-startup.ts",
  key,
  text: (lang) => startup(lang, key),
  later: startup.hi[key] === null,
});

export const breakWords = (key: BreakKey): WordsRef => ({
  file: "lib/words-breaks.ts",
  key,
  text: (lang) => brk(lang, key),
  later: brk.hi[key] === null,
});

export const settingsWords = (key: SettingsKey): WordsRef => ({
  file: "lib/words-settings.ts",
  key,
  text: (lang) => st(lang, key),
  later: st.hi[key] === null,
});
