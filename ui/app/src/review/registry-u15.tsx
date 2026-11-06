// U15's copy review entries: the Connectors section of the settings window (settings-app.tsx) over FakeTransport,
// open at Connectors, at the window's size, one state per row of the brief's Settings table. The connector's state
// comes in the app feed's catch-up, as the host sends it; the packs, the key field and the payment page show after
// the press that opens them, made once the button is on the page. Figures are review fixtures, not prices. The room's
// hint and banner (U15-58 to U15-66, U15-75) come with the room's part.
import { cw, type ConnectorsKey } from "@clauseo/components/lib/words-connectors.ts";
import type { ConnectorView, ConnectorsState, PackView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { PressWords } from "./press-words.tsx";
import { SettingsOver } from "./states-settings.tsx";
import type { WordsRef } from "./words.ts";

/** The settings window, 960 by 700 (DESIGN.md). */
const WINDOW = { width: 960, height: 700 } as const;

/** A string of the Connectors section (words-connectors.ts). */
const connectorsWords = (key: ConnectorsKey): WordsRef => ({
  file: "lib/words-connectors.ts",
  key,
  text: (lang) => cw(lang, key),
  later: cw.hi[key] === null,
});

const PACKS: PackView[] = [
  // gross prices, GST included: 200 credits for ₹23.60 (₹20 and ₹3.60 GST)
  { pack: "p200", credits: 200, paise: 2_360 },
  { pack: "p1k", credits: 1_000, paise: 11_800 },
  { pack: "p10l", credits: 10_00_000, paise: 1_18_00_000 },
];

const PAY = "https://data.clauseo.chat/pay/review";

const of = (
  over: Partial<ConnectorView> = {},
  top: Partial<Omit<ConnectorsState, "connectors">> = {},
): ConnectorsState => ({
  available: true,
  locked: false,
  ...top,
  connectors: [
    {
      id: "case-law",
      name: "Indian Kanoon",
      price_paise: 2_360,
      packs: PACKS,
      status: "not_added",
      ...over,
    },
  ],
});

const BALANCE: ConnectorView["status"] = {
  signed_in: { balance: { credits: 10_000, paise: 1_00_000 }, low: false },
};

/** Each row's state, and the button pressed once it shows (none when left out). */
const STATES = {
  "not-added": { state: of() },
  "signing-in": { state: of({ status: "signing_in" }) },
  balance: { state: of({ status: BALANCE }) },
  "balance-unread": { state: of({ status: { signed_in: { low: false } } }) },
  low: {
    state: of({ status: { signed_in: { balance: { credits: 180, paise: 1_800 }, low: true } } }),
  },
  spend: {
    state: of({
      status: BALANCE,
      recent_spend: {
        today_credits: 340,
        days: [
          { day: "2026-11-04", credits: 1_200 },
          { day: "2026-11-03", credits: 560 },
        ],
      },
    }),
  },
  "spend-none": {
    state: of({ status: BALANCE, recent_spend: { today_credits: 0, days: [] } }),
  },
  "out-of-credit": { state: of({ status: { out_of_credit: {} } }) },
  "signed-out": { state: of({ status: "signed_out" }) },
  "key-revoked": { state: of({ status: "key_revoked" }) },
  error: { state: of({ status: { error: { message: "sign-in callback timed out" } } }) },
  locked: { state: of({ status: BALANCE }, { available: false, locked: true }) },
  off: { state: of({ status: BALANCE }, { available: false, locked: false }) },
  "turn-off": { state: of({ status: BALANCE }) },
  packs: { state: of({ status: BALANCE }), press: "Add credits" },
  "pay-ready": { state: of({ status: BALANCE, pay_url: PAY }) },
  paying: { state: of({ status: BALANCE, pay_url: PAY }), press: "Open the payment page" },
  "api-key": { state: of(), press: "Use an API key instead" },
} satisfies Record<string, { state: ConnectorsState; press?: ConnectorsKey }>;

type Row = keyof typeof STATES;

const entry = (
  id: string,
  key: ConnectorsKey,
  row: Row,
  height: number = WINDOW.height,
): Entry => ({
  id,
  words: connectorsWords(key),
  screen: "settings/connectors",
  width: WINDOW.width,
  height,
  render: (lang) => {
    const made = STATES[row];
    const over = <SettingsOver section="connectors" lang={lang} connectors={made.state} />;
    return "press" in made ? <PressWords words={cw(lang, made.press)}>{over}</PressWords> : over;
  },
});

/** The section's entries, each id written as it is: an id never moves when an entry is added or taken out. U15-15 and
 * U15-16 are reserved: the balance's worth and rupees left arrive with stage E's gross per-credit rate, GST included
 * (the orchestrator's GST ruling of 2026-11-06); until then the balance and spend show in credits only. */
export const U15: Entry[] = [
  entry("U15-01", "Connectors", "not-added"),
  entry("U15-02", "Paid sources Clauseo can search while it answers.", "not-added"),
  entry("U15-03", "Indian Kanoon", "not-added"),
  entry(
    "U15-04",
    "Judgments of the Supreme Court, the High Courts and tribunals, with statutes.",
    "not-added",
  ),
  entry("U15-05", "Not added", "not-added"),
  entry("U15-06", "Credits are bought in packs, from {price}, including GST.", "not-added"),
  entry("U15-07", "Add", "not-added"),
  entry("U15-08", "Your browser opens to sign in.", "not-added"),
  entry("U15-09", "Use an API key instead", "not-added"),
  entry("U15-10", "Signing in", "signing-in"),
  entry("U15-11", "Finish signing in on the page your browser opened.", "signing-in"),
  entry("U15-12", "Open the sign-in page again", "signing-in"),
  entry("U15-13", "Ready to search.", "balance"),
  entry("U15-14", "{credits} credits left", "balance"),
  entry("U15-17", "Balance not read yet.", "balance-unread"),
  entry("U15-18", "Check balance", "balance-unread"),
  entry("U15-19", "Running low. Add credits to keep searching.", "low"),
  entry("U15-20", "Add credits", "balance"),
  entry("U15-21", "Sign out", "balance"),
  entry("U15-22", "Remove", "balance"),
  entry("U15-23", "Recent spend", "spend", 900),
  entry("U15-24", "Today", "spend", 900),
  entry("U15-25", "{credits} credits", "spend", 900),
  entry("U15-26", "Nothing spent yet.", "spend-none", 900),
  entry("U15-27", "Out of credit", "out-of-credit"),
  entry("U15-28", "No credits left. Case-law searches stop until you add more.", "out-of-credit"),
  entry("U15-29", "Signed out", "signed-out"),
  entry("U15-30", "Signed out. Sign in to search case law again.", "signed-out"),
  entry("U15-31", "Sign in", "signed-out"),
  entry("U15-32", "Key no longer works", "key-revoked"),
  entry(
    "U15-33",
    "This API key no longer works. Add a new key, or sign in instead.",
    "key-revoked",
  ),
  entry("U15-34", "Add a new key", "key-revoked"),
  entry("U15-35", "Sign-in failed", "error"),
  entry("U15-36", "The sign-in didn't go through. Try again, or use an API key instead.", "error"),
  entry("U15-37", "Try again", "error"),
  entry("U15-38", "Details", "error"),
  entry("U15-39", "Turned off by your firm", "locked"),
  entry("U15-40", "Turned off by your firm.", "locked"),
  entry("U15-41", "Choose a pack", "packs"),
  entry("U15-42", "{credits} credits, {rupees}", "packs"),
  entry("U15-43", "Prices include GST.", "packs"),
  entry("U15-44", "Continue to payment", "packs"),
  entry("U15-45", "Cancel", "packs"),
  entry("U15-46", "The payment page is open in your browser.", "paying"),
  entry("U15-47", "Your balance updates here once the payment goes through.", "paying"),
  entry("U15-48", "Open the payment page again", "paying"),
  entry("U15-49", "API key", "api-key"),
  entry(
    "U15-50",
    "Your firm's key for Clauseo's case-law service. Kept on this computer.",
    "api-key",
  ),
  entry("U15-51", "Add key", "api-key"),
  entry("U15-52", "What each search sends", "not-added"),
  entry(
    "U15-53",
    "The words of each search go to Clauseo's server, which sends them on to Indian Kanoon.",
    "not-added",
  ),
  entry(
    "U15-54",
    "Papers aren't sent. Clauseo asks {name} to keep client names and confidential facts out of its searches.",
    "not-added",
  ),
  entry("U15-55", "Clauseo's server deletes the words of each search after 7 days.", "not-added"),
  entry("U15-56", "Indian Kanoon sees Clauseo's server, never you.", "not-added"),
  entry("U15-57", "Each search and each judgment read costs credits.", "not-added"),
  entry("U15-67", "Turned off on this computer", "off"),
  entry("U15-68", "Turned off on this computer.", "off"),
  entry("U15-69", "Turn on", "off"),
  entry("U15-70", "On this computer", "turn-off", 900),
  entry("U15-71", "Case-law searches", "turn-off", 900),
  entry(
    "U15-72",
    "Turning off stops case-law searches on this computer. Your credits stay.",
    "turn-off",
    900,
  ),
  entry("U15-73", "Turn off", "turn-off", 900),
  entry("U15-74", "Open the payment page", "pay-ready"),
];
