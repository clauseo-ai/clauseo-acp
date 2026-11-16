// U11's copy review entries (the settings window), apart from registry.tsx to keep that file under the size limit:
// each string in the real window (settings-app.tsx) over FakeTransport, at the window's own size, open at the section
// that shows it. The locked row's note shows with the theme locked by a firm's policy.
import { TestBuildTag } from "@clauseo/components/clauseo/test-build.tsx";
import type { SectionId } from "../settings/sections.tsx";
import type { Entry } from "./entry.ts";
import { SettingsOver } from "./states-settings.tsx";
import { frameWords, settingsWords, uiWords, type WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

const entry = (id: string, words: WordsRef, section: SectionId, locked?: string[]): Entry => ({
  id,
  words,
  screen: `settings/${section}`,
  ...WINDOW,
  render: (lang) => (
    <SettingsOver section={section} lang={lang} {...(locked === undefined ? {} : { locked })} />
  ),
});

/** The release date the review shows About with: a release build's, as display_info reports it. */
const RELEASE_DATE_SHOWN = "2026-11-03";

/** U11's entries, each id written as it is: an id never moves when an entry is added or taken out. */
export const U11: Entry[] = [
  entry("U11-01", settingsWords("Sections"), "language"),
  entry("U11-02", settingsWords("Language"), "language"),
  entry("U11-03", settingsWords("Appearance"), "appearance"),
  entry("U11-04", settingsWords("About"), "about"),
  entry("U11-05", settingsWords("{theme}, text size {n}"), "appearance"),
  entry("U11-06", settingsWords("Version {version}"), "about"),
  entry("U11-07", settingsWords("Managed by firm"), "appearance", ["ui.theme"]),
  entry(
    "U11-08",
    settingsWords(
      "The language of every word on screen. Papers, quotes and case names stay as filed.",
    ),
    "language",
  ),
  entry("U11-09", settingsWords("How Clauseo looks on this computer."), "appearance"),
  entry("U11-10", settingsWords("Changes at once, and everything stays where it was."), "language"),
  entry("U11-11", settingsWords("Day"), "appearance"),
  entry("U11-12", settingsWords("Night"), "appearance"),
  entry("U11-13", frameWords("Theme"), "appearance"),
  entry("U11-14", frameWords("Follow the computer"), "appearance"),
  entry("U11-15", frameWords("Text size"), "appearance"),
  entry("U11-16", frameWords("Every word on screen grows or shrinks with it."), "appearance"),
  entry("U11-17", frameWords("Reduced motion"), "appearance"),
  entry("U11-18", frameWords("Every change lands at once, and nothing moves."), "appearance"),
  entry("U11-19", uiWords("Lite mode"), "appearance"),
  entry("U11-20", uiWords("Takes away every shadow and all motion."), "appearance"),
  entry("U11-21", uiWords("On"), "appearance"),
  entry("U11-22", uiWords("Always, on this computer"), "appearance"),
  entry("U11-23", uiWords("Off"), "appearance"),
  entry("U11-24", uiWords("Never, even over a remote desktop"), "appearance"),
  entry("U11-25", uiWords("Settings"), "language"),
  entry("U11-26", settingsWords("How it looks"), "appearance"),
  entry(
    "U11-27",
    settingsWords("A strong case, because the complaint names Mr. Sharma only as a director."),
    "appearance",
  ),
  // About's release line, with a release date the host reports (a release build's; a dev build shows no line)
  {
    ...entry("U11-50", settingsWords("Released {date}"), "about"),
    render: (lang) => <SettingsOver section="about" lang={lang} releaseDate={RELEASE_DATE_SHOWN} />,
  },
  // the Test build label, beside About's version in a test build (and at the rail's foot): shown as its own tag
  {
    ...entry("U11-51", uiWords("Test build"), "about"),
    render: (lang) => <TestBuildTag lang={lang} />,
  },
];
