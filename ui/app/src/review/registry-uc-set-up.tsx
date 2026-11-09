// UC's copy review entries for R17 PR-D3a: Settings' Safe workspace on a build that sets the Safe
// workspace up but doesn't start Claude or ChatGPT in it yet (`computer` stays `unsupported {
// not_built }`, and `set_up_state` says the set-up's own state). Provisional, English and Hindi.
import type { SettingsKey } from "@clauseo/components/lib/words-settings.ts";
import type { FakeProtectionHost } from "../transport/fake-transport.ts";
import type { Protection, ProtectionView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { SettingsOver } from "./states-settings.tsx";
import { settingsWords } from "./words.ts";

const np = (cause: unknown): Protection => ({ not_protected: { cause } }) as Protection;
const NOT_BUILT = np({ unsupported: { why: "not_built" } });

/** A host that sets up without starting Claude or ChatGPT in it: `set_up_state` beside not-built. */
const setUp = (state: Protection): FakeProtectionHost => ({
  view: () =>
    ({
      tier: "c",
      computer: NOT_BUILT,
      required: false,
      unprotected_auto_allowed: true,
      retry_asks_admin: false,
      set_up_state: state,
    }) as ProtectionView,
  command: () => new Promise<void>(() => {}),
});

const settings = (id: string, key: SettingsKey, protection: FakeProtectionHost): Entry => ({
  id,
  words: settingsWords(key),
  screen: "settings/workspace",
  width: 960,
  height: 720,
  render: (lang) => <SettingsOver section="workspace" lang={lang} protection={protection} />,
});

/** UC-101 to UC-104. */
export const UC_SET_UP: Entry[] = [
  settings(
    "UC-101",
    "Set up. Clauseo will start using it in an update.",
    setUp({ protected: { tier: "b" } } as Protection),
  ),
  settings(
    "UC-102",
    "Set up the Safe workspace now, so it's ready when Clauseo starts using it in an update.",
    setUp(np("needs_set_up")),
  ),
  settings(
    "UC-103",
    "Set up the Safe workspace again, so it's ready when Clauseo starts using it in an update. Windows didn't allow the last set-up.",
    setUp(np("set_up_refused")),
  ),
  settings(
    "UC-104",
    "Set up the Safe workspace again, so it's ready when Clauseo starts using it in an update. The last set-up didn't work.",
    setUp(np("set_up_failed")),
  ),
];
