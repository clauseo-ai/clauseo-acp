// U16a's copy review entries for the rail (Needs you, the status marks, Matters / Recent), apart from registry.tsx to
// keep that file under the size limit; Find's are registry-u16a-find.tsx's. Each renders in the real rail over
// FakeTransport (hosts.ts's railHost: every rail state, with every matter's threads as the host's first frames give
// them), at the rail's window. An id the approved page gives several lines ("Matters / Recent") takes them joined as the
// page writes them, each still looked up from its words file.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { frame, type FrameKey } from "@clauseo/components/lib/words-frame.ts";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { t } from "@clauseo/components/lib/words.ts";
import type { Entry } from "./entry.ts";
import { railHost } from "./hosts.ts";
import { RoomAfter, RoomOver } from "./states.tsx";
import { frameWords, type WordsRef } from "./words.ts";

/** The rail open beside the work, every row in view. */
const RAIL = { width: 1280, height: 1500 } as const;

/** Several lines of one id, joined with " / " as the approved page writes them. Hindi is later while any line's is. */
const joined = (
  file: string,
  parts: readonly { text: (lang: Lang) => string; later: boolean }[],
): WordsRef => ({
  file,
  key: parts.map((p) => p.text("en")).join(" / "),
  text: (lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});
const frameLine = (key: FrameKey) => ({
  text: (lang: Lang) => frame(lang, key),
  later: frame.hi[key] === null,
});

const needsYou = (id: string, words: WordsRef): Entry => ({
  id,
  words,
  screen: "matters/rail-needs-you",
  ...RAIL,
  render: () => <RoomOver host={railHost} />,
});

export const U16A_RAIL: Entry[] = [
  // Needs you under Pinned: a question, a permission, an answer ready and a failure, each with its conversation beneath
  needsYou("RO-01", frameWords("Needs you")),
  // Matters / Recent under Find; the existing words keep their approved Hindi
  needsYou(
    "RO-18",
    joined("lib/words.ts, lib/words-matters.ts", [
      { text: (lang) => t(lang, "Matters"), later: t.hi["Matters"] === null },
      { text: (lang) => mt(lang, "Recent"), later: mt.hi["Recent"] === null },
    ]),
  ),
  // Recent chosen: today's conversations, yesterday's, and the days before by weekday and date
  {
    id: "RO-19",
    words: joined("lib/words-frame.ts", [
      frameLine("Today"),
      frameLine("Yesterday"),
      { text: () => "{weekday} {d} {month}", later: false },
    ]),
    screen: "matters/rail-recent",
    ...RAIL,
    render: () => <RoomAfter host={railHost} steps={[{ press: "rail-switch-recent" }]} />,
  },
  // a failed answer, unseen: in Needs you, and on Recent's row under the pointer
  needsYou(
    "RO-23",
    joined("lib/words-frame.ts", [
      frameLine("Failed at {time}"),
      frameLine("Failed on {day} at {time}"),
    ]),
  ),
];
