// R18's matter-level copy review entries (R18L): Settings' "Remembered answers", full (every kind of
// row, a matter this window doesn't know, a target from a newer Clauseo), empty, and with one answer
// forgotten (its Forget pressed); the room's protection note on a switched-off matter, with its
// remembered list, "Switch it back on" and the line about the next start; the open matter's menu,
// protected (Switch off…) and switched off (back on), with "Use files from another matter…" and
// the list it opens, and "Remembered answers…"; and each refusal of a switch-off, sandbox.required and
// sandbox.unprotected_auto_forbidden and a pick of another matter refused with
// escalation.network_location, whose words (U3-err's) are entered again here for the state
// that shows them; a switch-off refused with sandbox.required says its own line. All over FakeTransport with R18's host scripted (fake-allowances.ts). A menu item
// is pressed by its words (press-words.tsx's PressText), since the menu's items carry no test id.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { isolate } from "@clauseo/components/lib/isolate.ts";
import { aw, type AllowancesKey } from "@clauseo/components/lib/words-allowances.ts";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import type { ReactNode } from "react";
import type { MatterInfo } from "../room/host-next.ts";
import {
  agentText,
  batch,
  event,
  header,
  reset,
  THREAD,
  userMessage,
  uuid,
} from "../testing/stream-script.ts";
import {
  fakeAllowances,
  withAllowances,
  type FakeAllowancesRefusals,
} from "../transport/fake-allowances.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type {
  ErrorCode,
  MatterAllowance,
  MatterSummary,
  Protection,
  ProtectionView,
} from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER } from "./hosts.ts";
import { PressInOrder, PressText } from "./press-words.tsx";
import { RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import { errorWords, type WordsRef } from "./words.ts";

/** A string of R18's matter-level parts (words-allowances.ts). */
const words = (key: AllowancesKey): WordsRef => ({
  file: "lib/words-allowances.ts",
  key,
  text: (lang) => aw(lang, key),
  later: aw.hi[key] === null,
});

const OTHER: MatterSummary = {
  matter: "00000000-0000-4000-8000-000000000002",
  name: "Rao v. Union of India",
  kind: "folder",
};
const GONE = "00000000-0000-4000-8000-000000000009";
const kept = (
  matter: string,
  n: number,
  target: MatterAllowance["target"],
  allowed = true,
): MatterAllowance => ({
  matter,
  allowance: uuid(n, 8),
  target,
  mode: "read_only",
  allowed,
  at: 0,
});
const OURS: MatterAllowance[] = [
  kept(MATTER.matter, 1, { folder: { path: "/Users/asha/Desktop/Exhibits" } }),
  kept(MATTER.matter, 2, { site: { host: "files-share.example" } }, false),
];
const FULL: MatterAllowance[] = [
  ...OURS,
  kept(MATTER.matter, 3, { folder: { path: "/Users/asha/Downloads" } }, false),
  kept(OTHER.matter, 4, { file: { path: "/Users/asha/Orders/2026-03.pdf" } }),
  kept(OTHER.matter, 5, { file: { path: "/Users/asha/Orders/draft.docx" } }, false),
  kept(OTHER.matter, 6, {
    other_matter_path: { matter: MATTER.matter, path: "Pleadings", is_file: false },
  }),
  kept(
    OTHER.matter,
    7,
    {
      other_matter_path: { matter: MATTER.matter, path: "Notes", is_file: false },
    },
    false,
  ),
  kept(GONE, 8, "a_target_from_a_newer_clauseo"),
];

const OFF = {
  not_protected: { cause: { switched_off: { at: Date.UTC(2026, 10, 6, 10) } } },
} as Protection;
const ON = { protected: { tier: "a" } } as Protection;
/** This computer's view: protected, the firm letting a matter switch it off. */
const VIEW: ProtectionView = {
  tier: "a",
  computer: ON,
  required: false,
  unprotected_auto_allowed: true,
};

/** Every note explained, so none opens by itself over a cell (the frames share one localStorage). */
const quiet = () =>
  localStorage.setItem(
    "clauseo.protection-explained",
    JSON.stringify({ computer: true, matters: [MATTER.matter] }),
  );

type RoomScene = { protection: Protection; refuse?: FakeAllowancesRefusals; press: string[] };

const ENTRIES = [
  userMessage(5, 1, "List the exhibits the plaint relies on."),
  agentText(6, 1, "The plaint relies on six exhibits, marked A to F."),
];

const roomHost = (s: RoomScene) => () => {
  quiet();
  const allow = fakeAllowances({
    matters: [MATTER, OTHER],
    allowances: OURS,
    off: s.protection === OFF ? [MATTER.matter] : [],
    ...(s.refuse === undefined ? {} : { refuse: s.refuse }),
  });
  const transport: FakeTransport = new FakeTransport(
    withAllowances(
      {
        query: (query) => {
          if (query === "list_matters")
            return {
              matters: [
                { ...MATTER, protection: s.protection },
                { ...OTHER, protection: ON },
              ],
            };
          if (typeof query === "object" && "list_threads" in query)
            return {
              threads:
                query.list_threads.matter === MATTER.matter
                  ? [{ thread: THREAD, matter: MATTER.matter, agent: "claude-acp", status: "idle" }]
                  : [],
            };
          throw new Error(`not scripted: ${JSON.stringify(query)}`);
        },
        protection: { view: () => VIEW },
        // the window has Settings, so the menu offers Remembered answers…
        openSettings: () => {},
        subscribe: (stream) => {
          if (stream.startsWith("thread:"))
            queueMicrotask(() =>
              transport.deliver(event(batch(1, [reset(ENTRIES, header())], undefined, stream))),
            );
        },
      },
      allow,
    ),
  );
  allow.bind(transport);
  return transport;
};

const MENU = `Actions for ${MATTER.name}`;
const room = (id: string, ref: WordsRef, screen: string, s: RoomScene): Entry => ({
  id,
  words: ref,
  screen,
  render: (lang) => (
    <PressText
      texts={s.press.map((p) =>
        // the menu's trigger and a matter's isolated name are pressed as they are; the rest by their words
        p === MENU || p.startsWith("\u2068") ? p : aw(lang, p as AllowancesKey),
      )}
    >
      <RoomOver host={roomHost(s)} />
    </PressText>
  ),
});

/** The note, opened from the composer's line on a switched-off matter. */
const NOTE: RoomScene = { protection: OFF, press: [] };
const note = (id: string, key: AllowancesKey): Entry => ({
  id,
  words: words(key),
  screen: "room/not-protected-note",
  render: () => (
    <PressInOrder press={['[data-testid="workspace-line"]']}>
      <RoomOver host={roomHost(NOTE)} />
    </PressInOrder>
  ),
});

type Shelf = "full" | "empty" | "forgotten";
const NAMED = [MATTER, OTHER] as MatterInfo[];

const settingsOf = (lang: Lang, shelf: Shelf): ReactNode => {
  const over = (
    <SettingsOver
      section="allowances"
      lang={lang}
      protection={{ view: () => VIEW }}
      allowances={fakeAllowances({
        allowances: shelf === "empty" ? [] : shelf === "full" ? FULL : OURS,
      })}
      matters={NAMED}
    />
  );
  // the first row's Forget pressed: the folder goes, and the line under the list stays
  return shelf === "forgotten" ? (
    <PressText
      texts={[aw(lang, "Forget {target}", { target: "\u2068/Users/asha/Desktop/Exhibits\u2069" })]}
    >
      {over}
    </PressText>
  ) : (
    over
  );
};

const settings = (id: string, key: AllowancesKey, shelf: Shelf = "full"): Entry => ({
  id,
  words: words(key),
  screen: "settings/allowances",
  width: 960,
  height: 900,
  render: (lang) => settingsOf(lang, shelf),
});

type ErrorKey = Parameters<typeof errorWords>[0];
const refused = (code: ErrorCode): WordsRef => errorWords(ERROR_WORDS[code] as ErrorKey);

/** R18's matter-level entries, each id written as it is. */
export const R18L: Entry[] = [
  settings("R18L-01", "Remembered answers"),
  settings("R18L-02", "The folders and sites each matter keeps your answer for."),
  settings(
    "R18L-03",
    "No remembered answers. When you allow or refuse a folder or site for a matter and keep the answer, it shows here.",
    "empty",
  ),
  settings(
    "R18L-04",
    "Forgetting a folder takes effect the next time Clauseo starts work in that matter.",
  ),
  settings("R18L-05", "A matter not on your list"),
  settings("R18L-06", "Folder, read only"),
  settings("R18L-07", "File, read only"),
  settings("R18L-08", "Folder"),
  settings("R18L-09", "File"),
  settings("R18L-10", "Site"),
  settings("R18L-11", "From another matter, read only"),
  settings("R18L-12", "From another matter"),
  settings("R18L-13", "From a newer version of Clauseo"),
  settings("R18L-14", "Allowed"),
  settings("R18L-15", "Not allowed"),
  settings("R18L-16", "Forget"),
  settings("R18L-17", "Forget {target}"),
  settings(
    "R18L-18",
    "Forgetting a folder takes effect the next time Clauseo starts work in that matter.",
    "forgotten",
  ),
  note("R18L-19", "Remembered for this matter"),
  note("R18L-20", "Switch it back on"),
  note("R18L-21", "It applies the next time Clauseo starts work in this matter."),
  room("R18L-22", words("Switch off the Safe workspace…"), "room/matter-menu", {
    protection: ON,
    press: [MENU],
  }),
  room("R18L-23", words("Switch the Safe workspace back on"), "room/matter-menu", {
    protection: OFF,
    press: [MENU],
  }),
  room("R18L-24", words("Use files from another matter…"), "room/matter-menu", {
    protection: ON,
    press: [MENU],
  }),
  room("R18L-25", words("Use files from another matter"), "room/other-matter", {
    protection: ON,
    press: [MENU, "Use files from another matter…"],
  }),
  room(
    "R18L-26",
    words(
      "Choose the matter. Clauseo then opens its folder so you can pick the files, which it can only read.",
    ),
    "room/other-matter",
    { protection: ON, press: [MENU, "Use files from another matter…"] },
  ),
  room("R18L-27", words("Cancel"), "room/other-matter", {
    protection: ON,
    press: [MENU, "Use files from another matter…"],
  }),
  room(
    "R18L-28",
    words("Your firm requires the Safe workspace in every matter, so it can't be switched off."),
    "room/matter-menu",
    {
      protection: ON,
      refuse: { switch_protection_off: "sandbox.required" },
      press: [MENU, "Switch off the Safe workspace…"],
    },
  ),
  room("R18L-29", refused("sandbox.unprotected_auto_forbidden"), "room/matter-menu", {
    protection: ON,
    refuse: { switch_protection_off: "sandbox.unprotected_auto_forbidden" },
    press: [MENU, "Switch off the Safe workspace…"],
  }),
  room("R18L-30", words("Remembered answers…"), "room/matter-menu", {
    protection: ON,
    press: [MENU],
  }),
  // a pick the host refuses because the other matter's folder is on a network location
  room("R18L-31", refused("escalation.network_location"), "room/matter-menu", {
    protection: ON,
    refuse: { files_from_other_matter: "escalation.network_location" },
    press: [MENU, "Use files from another matter…", isolate(OTHER.name)],
  }),
];
