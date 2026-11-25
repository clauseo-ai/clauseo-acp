// The keychain row's pointer (UC-1440 to UC-1442, and UC-1444 for #1348's Remove that left the sign-in in the keychain;
// stage E, Mac keychain prompting off): the room over library-host.ts
// with Case Law signed out and the desktop listing the keychain row as would_ask, so the band stands with its Allow
// and Skills and connectors point at it in place of Signed out and the sign-in line (room/first-run/keychain-pointer.tsx).
// UC-1500: the band's "Not now" on a keychain the lawyer refused. U3-err-agent.key_denied.windows_linux: agent.key_denied
// off the Mac, where there is no keychain row to point at.
import type { Lang } from "@clauseo/components/lib/display.ts";
import {
  errorWords as sentence,
  KEY_DENIED_ELSEWHERE,
  onComputer,
} from "@clauseo/components/lib/words-errors.ts";
import { cw } from "@clauseo/components/lib/words-connectors.ts";
import { fl, type FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import { providePermissions } from "../../room/first-run/permissions-host.ts";
import type { Entry } from "../entry.ts";
import { libraryHost } from "../library-host.ts";
import { RoomClicking } from "../states.tsx";
import { SettingsOver } from "../states-settings.tsx";
import { errorWords, type WordsRef } from "../words.ts";

const words = (key: FirstLaunchKey): WordsRef => ({
  file: "lib/words-first-launch.ts",
  key,
  text: (lang) => fl(lang, key),
  later: fl.hi[key] === null,
});

const KEYS = {
  id: "keychain",
  state: "would_ask",
  folder: null,
  settings_path: null,
  note: null,
} as const;

/** Case Law signed out, the keychain row waiting (or refused: `state`, its ask answered `after`), and `clicks` pressed. */
const over =
  (
    clicks: string[],
    after: "granted" | "would_ask" = "granted",
    state: "would_ask" | "denied" = "would_ask",
  ) =>
  () => (
    <RoomClicking
      host={() => {
        const transport = libraryHost({ caseLaw: "ended" });
        providePermissions(transport, {
          list: async () => [{ ...KEYS, state }],
          ask: async () => ({ ...KEYS, state: after }),
        });
        return transport;
      }}
      clicks={clicks}
    />
  );
/** The place opened at Connectors, then `clicks`. */
const at = (...clicks: string[]) =>
  over(['[data-testid="library-entry"]', '[role="tab"]:nth-of-type(2)', ...clicks]);

const entry = (id: string, key: FirstLaunchKey, render: Entry["render"]): Entry => ({
  id,
  words: words(key),
  screen: "room/library",
  render,
});

const LEFT = "Removed from Clauseo. The saved sign-in stays in your Mac's keychain.";

const UC_KEYCHAIN_POINTER: Entry[] = [
  entry(
    "UC-1440",
    "Clauseo needs your Mac's permission to read its saved keys.",
    at('[data-testid="case-law-card"] .lib-feature-hit'),
  ),
  entry("UC-1441", "Needs your Mac's permission", at()),
  // the band's Allow answered with macOS's plain Allow: the row still would ask
  entry(
    "UC-1442",
    "macOS allowed it once only. Press Allow again and choose Always Allow.",
    over(['[data-testid="permission-allow"]'], "would_ask"),
  ),
  // a keychain refused at an earlier start: the band stands with Allow and Not now, which puts it away until next start
  entry("UC-1500", "Not now", over([], "granted", "denied")),
  {
    id: "U3-err-agent.key_denied.windows_linux",
    words: errorWords(KEY_DENIED_ELSEWHERE),
    screen: "room/library",
    render: (lang: Lang) => (
      <main lang={lang} className="review-alone">
        <p>{sentence(lang, "agent.key_denied", undefined, onComputer("windows"))}</p>
      </main>
    ),
  },
  // UC-1444: Remove's outcome in Settings › Connectors while the keychain kept the saved sign-in (#1348's field)
  {
    id: "UC-1444",
    words: {
      file: "lib/words-connectors.ts",
      key: LEFT,
      text: (lang) => cw(lang, LEFT),
      later: cw.hi[LEFT] === null,
    },
    screen: "settings/connectors",
    width: 960,
    height: 700,
    render: (lang) => (
      <SettingsOver
        section="connectors"
        lang={lang}
        connectors={{
          available: true,
          locked: false,
          connectors: [
            {
              id: "case-law",
              name: "Indian Kanoon",
              packs: [],
              status: "not_added",
              left_in_keychain: true,
            },
          ],
        }}
      />
    ),
  },
];

export default UC_KEYCHAIN_POINTER;
