// The unsafe-site list's copy review entries (0053, UBL): the notice under the answer of the turn a
// site was blocked in, in the real room over FakeTransport with the notice on the app feed as the
// host sends it: blocked with both allows (phishing), blocked three times (malware), the firm's no
// override, allowed for the matter and always, and the refusals (override_off, an expired notice),
// each after the presses that lead there (PressInOrder); the Allow confirm for each reach; and
// Settings, Privacy's allowed sites, listed and empty. The no-override and override_off lines are
// U3-err-blocklist.override_off's words and the expired one U3-err-blocklist.notice_expired's,
// entered again here for the states that show them.
import { bl, type BlocklistKey } from "@clauseo/components/lib/words-blocklist.ts";
import { ERROR_WORDS } from "@clauseo/components/lib/words-errors.ts";
import type { ReactNode } from "react";
import type { MatterInfo, SiteBlocked } from "../room/host-next.ts";
import { agentText, step, THREAD, TURN, userMessage, uuid } from "../testing/stream-script.ts";
import { fakeBlocklist } from "../transport/fake-blocklist.ts";
import type { AllowedSite, ErrorCode } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { MATTER, threadHost } from "./hosts.ts";
import { PressInOrder } from "./press-words.tsx";
import { RoomOver } from "./states.tsx";
import { SettingsOver } from "./states-settings.tsx";
import { errorWords, type WordsRef } from "./words.ts";

/** A string of the unsafe-site list's parts (words-blocklist.ts). */
const words = (key: BlocklistKey): WordsRef => ({
  file: "lib/words-blocklist.ts",
  key,
  text: (lang) => bl(lang, key),
  later: bl.hi[key] === null,
});

const site = (over: Partial<SiteBlocked> = {}): SiteBlocked => ({
  block_id: uuid(9, 7),
  matter: MATTER.matter,
  thread: THREAD,
  turn: TURN,
  host: "invoice-paypa1.example",
  tag: "phishing",
  list_generated: Date.UTC(2026, 10, 1, 6),
  override_allowed: true,
  count: 1,
  ...over,
});

/** The open conversation: a question whose answer's fetch the list refused. */
const ENTRIES = [
  userMessage(5, 1, "Check the vendor's payment page for the invoice in clause 4."),
  agentText(
    6,
    1,
    "The vendor's payment page didn't open, so the invoice couldn't be checked there.",
  ),
  step(9),
];

type Scene = { sites: SiteBlocked[]; refuse?: ErrorCode; press?: string[] };

const SCENES = {
  blocked: { sites: [site()] },
  repeated: { sites: [site({ tag: "malware", host: "update-cdn.example", count: 3 })] },
  "no-override": { sites: [site({ override_allowed: false })] },
  "confirm-matter": { sites: [site()], press: ['[data-testid="site-allow-matter"]'] },
  "confirm-always": { sites: [site()], press: ['[data-testid="site-allow-always"]'] },
  "allowed-matter": {
    sites: [site()],
    press: ['[data-testid="site-allow-matter"]', '[data-testid="site-allow-ok"]'],
  },
  "allowed-always": {
    sites: [site()],
    press: ['[data-testid="site-allow-always"]', '[data-testid="site-allow-ok"]'],
  },
  "override-off": {
    sites: [site()],
    refuse: "blocklist.override_off",
    press: ['[data-testid="site-allow-matter"]', '[data-testid="site-allow-ok"]'],
  },
  expired: {
    sites: [site()],
    refuse: "blocklist.notice_expired",
    press: ['[data-testid="site-allow-matter"]', '[data-testid="site-allow-ok"]'],
  },
} satisfies Record<string, Scene>;

const roomOf = (scene: Scene): ReactNode => {
  const over = (
    <RoomOver
      host={() =>
        threadHost({
          entries: ENTRIES,
          blocked: {
            sites: scene.sites,
            ...(scene.refuse === undefined ? {} : { refuse: scene.refuse }),
          },
        })
      }
    />
  );
  return scene.press === undefined ? over : <PressInOrder press={scene.press}>{over}</PressInOrder>;
};

const room = (id: string, ref: WordsRef, scene: keyof typeof SCENES): Entry => ({
  id,
  words: ref,
  screen: scene.startsWith("confirm") ? "room/site-allow-confirm" : "room/site-blocked",
  render: () => roomOf(SCENES[scene]),
});

const AT = Date.UTC(2026, 10, 5, 6);
const SITES: AllowedSite[] = [
  { host: "update-cdn.example", scope: "always", allowed_at: AT },
  { host: "invoice-paypa1.example", scope: "matter", matter: MATTER.matter, allowed_at: AT },
  // a forgotten matter's allow, kept
  { host: "old-portal.example", scope: "matter", matter: uuid(5, 2), allowed_at: AT },
];
const NAMED = [{ ...MATTER } as MatterInfo];

const settings = (id: string, key: BlocklistKey, empty = false): Entry => ({
  id,
  words: words(key),
  screen: "settings/privacy",
  width: 960,
  height: 900,
  render: (lang) => (
    <SettingsOver
      section="privacy"
      lang={lang}
      blocklist={fakeBlocklist({ sites: empty ? [] : SITES })}
      matters={NAMED}
    />
  ),
});

const OFF = errorWords(ERROR_WORDS["blocklist.override_off"] as Parameters<typeof errorWords>[0]);
const EXPIRED = errorWords(
  ERROR_WORDS["blocklist.notice_expired"] as Parameters<typeof errorWords>[0],
);

/** The unsafe-site list's entries, each id written as it is. */
export const UBL: Entry[] = [
  room("UBL-01", words("Unsafe site blocked"), "blocked"),
  room("UBL-02", words("Clauseo blocked {host}."), "blocked"),
  room("UBL-03", words("This site is linked to harmful software."), "repeated"),
  room("UBL-04", words("This site may try to steal passwords or details."), "blocked"),
  room("UBL-05", words("Blocked {n} times in this answer."), "repeated"),
  room("UBL-06", words("It's on Clauseo's unsafe-site list."), "blocked"),
  room("UBL-07", words("List of {date}"), "blocked"),
  room("UBL-08", words("Allow for this matter"), "blocked"),
  room("UBL-09", words("Always allow"), "blocked"),
  room("UBL-10", words("Allowed for this matter"), "allowed-matter"),
  room("UBL-11", words("Allowed always"), "allowed-always"),
  room("UBL-13", words("Allow {host} for this matter?"), "confirm-matter"),
  room("UBL-14", words("Always allow {host}?"), "confirm-always"),
  room(
    "UBL-15",
    words("From now on, Clauseo can open this site in this matter."),
    "confirm-matter",
  ),
  room(
    "UBL-16",
    words("From now on, Clauseo can open this site in every matter."),
    "confirm-always",
  ),
  room("UBL-17", words("You can take this back in Settings, under Privacy."), "confirm-matter"),
  room("UBL-18", words("Allow"), "confirm-matter"),
  settings("UBL-19", "Allowed sites"),
  settings("UBL-20", "Sites you allowed although they're on the unsafe-site list."),
  settings("UBL-21", "No sites are allowed past the unsafe-site list.", true),
  settings("UBL-22", "This matter: {matter}"),
  settings("UBL-23", "One matter"),
  settings("UBL-24", "Always"),
  settings("UBL-25", "Allowed {date}"),
  settings("UBL-26", "Remove"),
  settings("UBL-27", "Remove {host}"),
  room("UBL-28", OFF, "no-override"),
  room("UBL-29", OFF, "override-off"),
  room("UBL-30", EXPIRED, "expired"),
];
