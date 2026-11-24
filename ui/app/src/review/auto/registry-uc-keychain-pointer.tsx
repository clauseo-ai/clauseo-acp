// The keychain row's pointer (UC-1440 to UC-1443; stage E, Mac keychain prompting off): the room over library-host.ts
// with Case Law signed out and the desktop listing the keychain row as would_ask, so the band stands with its Allow
// and Skills and connectors point at it in place of Signed out and the sign-in line (room/first-run/keychain-pointer.tsx).
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

/** Case Law signed out, the keychain row waiting (its ask answered `after`), and `clicks` pressed. */
const over =
  (clicks: string[], after: "granted" | "would_ask" = "granted") =>
  () => (
    <RoomClicking
      host={() => {
        const transport = libraryHost({ caseLaw: "ended" });
        providePermissions(transport, {
          list: async () => [KEYS],
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
];

// agent.key_denied's words for a read, a save or a removal, in place of the retired U3-err-agent.key_denied; Plans,
// where they show with no permissions read (the settings window lists no rows)
UC_KEYCHAIN_POINTER.push({
  id: "UC-1443",
  words: errorWords(
    "This computer's keychain didn't let Clauseo use the saved API key. On a Mac, press Allow on Your keychain, then choose Always Allow.",
  ),
  screen: "settings/plans",
  width: 960,
  height: 1000,
  render: (lang) => (
    <SettingsOver
      section="plans"
      lang={lang}
      setup={{
        first: [
          {
            agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } },
          },
          {
            sign_in_changed: {
              agent: "claude-acp",
              state: { failed: { code: "agent.key_denied", at: 0 } },
            },
          },
        ],
      }}
    />
  ),
});

export default UC_KEYCHAIN_POINTER;
