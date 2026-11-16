// The words of the first run (UD1): the first screen, the agent getting ready at its foot and in the composer, and
// signing in where the composer's question will go. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language. The strings from the approved design
// (design/03-system/src/lib/words-first-launch.ts, interface strings only, never its sample matter's) keep the
// design's Hindi. The rest are new, drafted in the design's voice in English (QU.8): hi: later. Three of the design's
// sentences are the setup codes' own (agent.install_offline, agent.install_blocked, signin.cancelled), so the screens
// say them through words-errors.ts and they are not repeated here. Nothing names a model, and nothing says "agent".
import { useDisplay, type Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-first-launch.hi.ts";

/** Every string of the first run, in English. */
export type FirstLaunchKey = keyof typeof HI;
/** A first-run string in the language, its slots filled. */
export const fl = wordsOf("first-launch");

/** The first run's words in the page's language: const w = useFirstLaunchWords(); w("Sign in"). */
export function useFirstLaunchWords() {
  const { lang } = useDisplay();
  return (key: FirstLaunchKey, slots?: Slots) => fl(lang, key, slots);
}

// The note "Copy details for IT" copies (clauseo/copy-for-it.tsx): one line saying what Clauseo
// needs, then key: value facts, in the page's language as UD1-15's download note is. Only facts that
// hold no secret go in: the code and the reason the host gave, the site a list blocked, the
// addresses to allow, the program's path and hash where a view carries them, and Clauseo's version.
// Never a matter's path, a file's name, an email, a token or an account's name: a surface's builder
// hands over only these fields. The note ends with the case's section of Clauseo for firm IT
// (docs/firm-it.md, FIRM_IT_DOC), whose label is said in the language and whose URL never is. Here, in the first load with the button's words, so the lazy chunks that build notes
// (the thread's, the protection note's, Settings') share no chunk for it (vite-config.test.ts).

/** Clauseo for firm IT, on GitHub: docs/firm-it.md. */
export const FIRM_IT_DOC = "https://github.com/clauseo-ai/clauseo-acp/blob/main/docs/firm-it.md";

/** Each case's section of FIRM_IT_DOC (stage F's anchors), by the host's reason or code. Any other
 * code, a reason from a newer Clauseo included, takes the set-up's section. tool_missing and the
 * admin copy's blocks offer no button yet; their sections are here for the surfaces that will. */
const FIRM_IT_ANCHORS: Readonly<Record<string, string>> = {
  policy: "sandbox-policy",
  logon_right: "sandbox-logon-right",
  other_account: "sandbox-other-account",
  "sandbox.blocked": "sandbox-setup",
  "sandbox.required": "sandbox-required",
  "blocklist.override_off": "unsafe-site-override",
  tool_missing: "sandbox-tool-missing",
  admin_copy_missing: "sandbox-admin-copy",
  admin_copy_changed: "sandbox-admin-copy",
  admin_copy_unsafe: "sandbox-admin-copy",
};

/** The section of Clauseo for firm IT for a reason or a code; #sandbox-setup for any other. */
export const firmItLink = (code: string): string =>
  `${FIRM_IT_DOC}#${Object.hasOwn(FIRM_IT_ANCHORS, code) ? FIRM_IT_ANCHORS[code] : "sandbox-setup"}`;

/** The facts a note may hold; each left out says nothing. */
export type ItFacts = {
  /** An error code, such as sandbox.required. */
  code?: string | undefined;
  /** The host's reason, such as policy. */
  reason?: string | undefined;
  /** The site a list blocked, as the line shows it. */
  site?: string | undefined;
  /** Addresses the firm's network must allow. */
  hosts?: readonly string[] | undefined;
  /** The program the host names, and its hash, where its view carries them. */
  program?: string | undefined;
  hash?: string | undefined;
  /** Clauseo's version. */
  version: string;
  /** The case's section of Clauseo for firm IT (firmItLink), the note's last line. */
  link: string;
};

/** The note: the lead line, one line per fact given, then the link to Clauseo for firm IT. */
export function itNote(lang: Lang, lead: string, facts: ItFacts): string {
  const line = (key: Parameters<typeof fl>[1], value: string | undefined, slot: string) =>
    value === undefined || value === "" ? [] : [fl(lang, key, { [slot]: value })];
  return [
    lead,
    ...line("Code: {code}", facts.code, "code"),
    ...line("Reason: {reason}", facts.reason, "reason"),
    ...line("Site: {site}", facts.site, "site"),
    ...line("Addresses: {list}", facts.hosts?.join(", "), "list"),
    ...line("Program: {path}", facts.program, "path"),
    ...line("Program hash: {hash}", facts.hash, "hash"),
    ...line("Clauseo version: {version}", facts.version, "version"),
    ...line("Clauseo for firm IT: {url}", facts.link, "url"),
  ].join("\n");
}
