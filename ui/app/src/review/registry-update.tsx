// The updater's copy review entries (R23, decision record 0037). The room's notice in the real room over FakeTransport,
// its updater scripted (fake-update.ts): ready in auto and in notify mode, ready while a turn runs in a matter,
// scheduled after the answer, installing, and once after an update. Then Settings, Updates in each case: up to date
// after a check (with the mode choice), notify chosen, checking, downloading, ready, a failed install (the words of
// U3-err-update.failed, entered again for the state that shows them), a failed check, each kind of managed install,
// and the mode locked by policy.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import { st, type SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import { uw, type UpdateKey } from "@clauseo/components/lib/words-update.ts";
import type { ReactNode } from "react";
import { detailed, fakeNext } from "../room/fake-next.ts";
import { THREAD } from "../testing/stream-script.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { fakeUpdate } from "../transport/fake-update.ts";
import type { UpdateState, UpdateView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import { errorWords, type WordsRef } from "./words.ts";

/** A string of the updater's words (words-update.ts). */
const words = (key: UpdateKey): WordsRef => ({
  file: "lib/words-update.ts",
  key,
  text: (lang) => uw(lang, key),
  later: uw.hi[key] === null,
});

/** A settings string (words-settings.ts). */
const settingsWords = (key: SettingsKey): WordsRef => ({
  file: "lib/words-settings.ts",
  key,
  text: (lang) => st(lang, key),
  later: st.hi[key] === null,
});

const CHECKED = Date.UTC(2026, 10, 8, 4, 30);
const view = (state: UpdateState, over: Partial<UpdateView> = {}): UpdateView => ({
  current: "0.4.0",
  state,
  mode: "auto",
  checked_at: CHECKED,
  ...over,
});

type Notice = { view: UpdateView; running?: boolean };

const NOTICES = {
  "ready-auto": { view: view({ ready: { version: "0.4.1" } }) },
  "ready-notify": { view: view({ ready: { version: "0.4.1" } }, { mode: "notify" }) },
  "ready-running": { view: view({ ready: { version: "0.4.1" } }), running: true },
  scheduled: { view: view({ scheduled: { version: "0.4.1" } }), running: true },
  installing: { view: view({ installing: { version: "0.4.1" } }) },
  updated: { view: view("idle", { current: "0.4.1", updated_from: "0.4.0" }) },
} satisfies Record<string, Notice>;

const roomOf = ({ view: shown, running = false }: Notice): ReactNode => (
  <RoomOver
    host={() =>
      new FakeTransport({
        next: fakeNext({
          matters: () => [detailed(MATTER)],
          frames: true,
          threads: () =>
            running
              ? [{ thread: THREAD, matter: MATTER.matter, agent: "claude-acp", status: "running" }]
              : [],
        }),
        update: fakeUpdate({ view: shown }),
      })
    }
  />
);

const room = (id: string, ref: WordsRef, notice: keyof typeof NOTICES): Entry => ({
  id,
  words: ref,
  screen: "room/update-notice",
  width: 1280,
  render: () => roomOf(NOTICES[notice]),
});

const SETTINGS = {
  "up-to-date": { view: view("idle") },
  notify: { view: view("idle", { mode: "notify" }) },
  checking: { view: view("checking") },
  downloading: { view: view({ downloading: { version: "0.4.1", received: 1_200_000 } }) },
  ready: { view: view({ ready: { version: "0.4.1" } }) },
  "failed-install": {
    view: view({ failed: { version: "0.4.1", at: CHECKED, stage: "install" } }),
  },
  "failed-check": { view: view({ failed: { at: CHECKED, stage: "check" } }) },
  policy: { view: view({ managed: { by: "policy" } }, { mode: "none" }) },
  per_machine: { view: view({ managed: { by: "per_machine" } }) },
  read_only_location: { view: view({ managed: { by: "read_only_location" } }) },
  package: { view: view({ managed: { by: "package" } }) },
  locked: { view: view("idle", { mode: "notify" }), locked: ["update.mode"] },
} satisfies Record<string, { view: UpdateView; locked?: string[] }>;

const settingsOf = (lang: Lang, at: keyof typeof SETTINGS): ReactNode => {
  const shown: { view: UpdateView; locked?: string[] } = SETTINGS[at];
  return (
    <SettingsOver
      section="updates"
      lang={lang}
      locked={shown.locked ?? []}
      update={fakeUpdate({ view: shown.view })}
    />
  );
};

const settings = (id: string, ref: WordsRef, at: keyof typeof SETTINGS): Entry => ({
  id,
  words: ref,
  screen: "settings/updates",
  width: 960,
  height: 700,
  render: (lang) => settingsOf(lang, at),
});

const FAILED = errorWords(ERROR_WORDS["update.failed"] as Parameters<typeof errorWords>[0]);

/** The updater's entries, each id written as it is. */
export const R23: Entry[] = [
  room("R23-01", words("Clauseo {version} is ready to install."), "ready-auto"),
  room("R23-02", words("It installs the next time Clauseo opens."), "ready-auto"),
  room("R23-03", words("Restart to update"), "ready-auto"),
  room("R23-04", words("Restart to update"), "ready-notify"),
  room("R23-05", words("After this answer"), "ready-running"),
  room("R23-06", words("Restarting now stops the answer in progress."), "ready-running"),
  room("R23-07", words("Clauseo will restart to update after this answer."), "scheduled"),
  room("R23-08", words("Cancel"), "scheduled"),
  room("R23-09", words("Installing the update…"), "installing"),
  room("R23-10", words("Clauseo was updated from {from} to {current}."), "updated"),
  room("R23-11", words("Got it"), "updated"),
  settings("R23-12", words("Updates"), "up-to-date"),
  settings("R23-13", words("Clauseo {version}"), "up-to-date"),
  settings("R23-14", words("Last checked {time}"), "up-to-date"),
  settings("R23-15", words("When an update is ready"), "up-to-date"),
  settings("R23-16", words("Install updates when Clauseo next opens"), "up-to-date"),
  settings("R23-17", words("Tell me first"), "notify"),
  settings("R23-18", words("Check now"), "up-to-date"),
  settings("R23-19", words("Checking for updates…"), "checking"),
  settings("R23-20", words("Downloading the update…"), "downloading"),
  settings("R23-21", words("An update is ready. Restart to update from the main window."), "ready"),
  settings("R23-22", words("Clauseo is up to date."), "up-to-date"),
  settings("R23-23", FAILED, "failed-install"),
  settings("R23-24", words("Clauseo couldn't check for updates. Try again."), "failed-check"),
  settings("R23-25", words("Your firm manages updates for Clauseo."), "policy"),
  settings(
    "R23-26",
    words(
      "Clauseo is installed for everyone on this computer, so whoever looks after it installs updates.",
    ),
    "per_machine",
  ),
  settings(
    "R23-27",
    words(
      "Clauseo can't update itself from where it's installed. Ask whoever looks after this computer.",
    ),
    "read_only_location",
  ),
  settings(
    "R23-28",
    words("Updates come through the package manager that installed Clauseo."),
    "package",
  ),
  settings("R23-29", settingsWords("Managed by firm"), "locked"),
];
