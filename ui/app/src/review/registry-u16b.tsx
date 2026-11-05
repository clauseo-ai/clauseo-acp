// U16b's copy review entries (RO-02 to RO-17, RO-33): snooze, Done and the Snoozed shelf in the real rail over
// FakeTransport, apart from registry.tsx to keep that file under the size limit. The host lists Sharma v. State of Delhi
// open, with a conversation waiting, one with an answer ready, one snoozed and three done; Joshi v. Joshi snoozed whole;
// and a conversation of Kapoor's snoozed. Each state is taken as the lawyer would: the pointer to a row (its buttons
// shown with the focus, as the keyboard has them), a fold or a menu opened, a time chosen. The snooze menu's states
// (before and after 5:00 pm, with and without Before the next date) stand alone, its times worked out for 4:00 pm and
// 6:00 pm today, so the page shows each whatever the hour it's opened at.
import type { ReactNode } from "react";
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { TriageKey } from "@clauseo/components/lib/words-triage.ts";
import type { TriageLateKey } from "@clauseo/components/lib/words-triage-late.ts";
import { t } from "@clauseo/components/lib/words.ts";
import type { Entry } from "./entry.ts";
import { MenuAt, RoomTriage, type Act } from "./triage-states.tsx";
import { triageLateWords, triageWords, type WordsRef } from "./words.ts";

/** The rail open beside the work, every row in view. */
const RAIL = { width: 1280, height: 1100 } as const;

const ARM: readonly Act[] = [
  { arm: "Quashing petition, first draft" },
  { focus: '[data-testid="snooze"]' },
];
const SHELF: readonly Act[] = [{ press: '[data-testid="rail-shelf"] .room-fold' }];
const FOLD: readonly Act[] = [{ press: '[data-testid="done-fold"] .room-fold' }];
const SNOOZED: readonly Act[] = [
  { arm: "Quashing petition, first draft" },
  { press: '[data-testid="snooze"]' },
  { item: "Tomorrow" },
];
const MARKED: readonly Act[] = [
  { arm: "Quashing petition, first draft" },
  { press: '[data-testid="mark-done"]' },
];
const MATTER_MENU: readonly Act[] = [
  { press: '[aria-label="Actions for Sharma v. State of Delhi"]' },
  { focus: '[role="menuitem"]' },
];

/** Several lines of one id, joined with " / " as the approved page writes them; Hindi is later for all. */
const joined = (parts: readonly WordsRef[]): WordsRef => ({
  file: [...new Set(parts.map((p) => p.file))].join(", "),
  key: parts.map((p) => p.key).join(" / "),
  text: (lang: Lang) => parts.map((p) => p.text(lang)).join(" / "),
  later: parts.some((p) => p.later),
});
const undo: WordsRef = {
  file: "lib/words.ts",
  key: "Undo",
  text: (lang) => t(lang, "Undo"),
  later: t.hi["Undo"] === null,
};
const w = (key: TriageKey) => triageWords(key);
const wl = (key: TriageLateKey) => triageLateWords(key);
const entry = (id: string, words: WordsRef, render: () => ReactNode): Entry => ({
  id,
  words,
  screen: "matters/rail-triage",
  ...RAIL,
  render,
});

export const U16B: Entry[] = [
  // the shelf folded at the list's foot: Snoozed, its count, and when the next is back
  entry("RO-02", w("Snoozed"), () => <RoomTriage />),
  entry("RO-03", w("Next back {when}"), () => <RoomTriage />),
  // the shelf open: each row's time, Joshi v. Joshi snoozed whole, Wake now, and behind the info icon why sooner
  entry("RO-04", wl("Back sooner if Clauseo needs you or an answer finishes."), () => (
    <RoomTriage acts={[...SHELF, { hover: '[data-testid="rail-shelf"] .room-fold-info button' }]} />
  )),
  entry("RO-05", wl("Until {when}"), () => <RoomTriage acts={SHELF} />),
  entry("RO-06", wl("Whole matter"), () => <RoomTriage acts={SHELF} />),
  // the snooze menu: before 5:00 pm with Before the next date, after 5:00 pm without
  entry("RO-07", wl("Snooze until"), () => <MenuAt hour={16} nextIn={8} />),
  entry(
    "RO-08",
    joined([wl("Tomorrow {time}"), wl("Monday {time}"), wl("Choose a day and time")]),
    () => <MenuAt hour={18} />,
  ),
  entry("RO-09", wl("Before the next date"), () => <MenuAt hour={16} nextIn={8} />),
  // in the row's place after a snooze, with Undo; a snoozed conversation in its matter, its words under the pointer
  entry("RO-10", joined([w("Snoozed until {when}"), undo]), () => <RoomTriage acts={SNOOZED} />),
  entry("RO-11", w("Snoozed until {when}"), () => (
    <RoomTriage acts={[{ arm: "Witness statements" }, { focus: '[data-testid="snooze"]' }]} />
  )),
  entry("RO-12", wl("Wake now"), () => (
    <RoomTriage acts={[...SHELF, { focus: '[data-testid="wake-now"]' }]} />
  )),
  // a row's two buttons with the focus; Mark as not done on a done row, the fold open
  entry("RO-13", joined([wl("Snooze"), wl("Mark as done"), wl("Mark as not done")]), () => (
    <RoomTriage acts={ARM} />
  )),
  entry("RO-14", joined([w("Marked as done"), undo]), () => <RoomTriage acts={MARKED} />),
  // a matter's menu
  entry("RO-15", joined([wl("Snooze until"), wl("Mark all done")]), () => (
    <RoomTriage acts={MATTER_MENU} />
  )),
  // the Done fold, folded, then open with its explanation behind the info icon
  entry("RO-16", w("Done"), () => <RoomTriage />),
  entry(
    "RO-17",
    wl("Conversations you've read, with nothing waiting, move here each morning."),
    () => (
      <RoomTriage acts={[...FOLD, { hover: '[data-testid="done-fold"] .room-fold-info button' }]} />
    ),
  ),
  // a time tomorrow, in the line and on the shelf
  entry("RO-33", w("tomorrow {time}"), () => <RoomTriage acts={SNOOZED} />),
];
