// "Copy details for IT" on the firm-only lines (copy-for-it.tsx), shown only under a firm's policy:
// the button on each surface that offers it (the Not protected note of a policy block, the firm's notice over it, Settings' Safe
// workspace, and the unsafe-site notice where the firm forbids overriding it, before and after a
// refused allow), its copied state, and the words of the note it copies (it-details.ts,
// words-first-launch.ts's itNote), each on the state whose button copies them. The button's words
// are first-launch's, and its copied state UD1-16's, entered again here for these states. Built from
// UC's and UBL's scenes.
import type { BlocklistKey } from "@clauseo/components/lib/words-blocklist.ts";
import type { FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { ProtectionKey } from "@clauseo/components/lib/words-protection.ts";
import type { Protection, ProtectionView } from "../transport/protocol.ts";
import type { Entry } from "./entry.ts";
import { room as blocklistRoom, words as blocklistWords } from "./registry-blocklist.tsx";
import { held, LINE, note, P, room, settings, type RoomScene } from "./registry-uc.tsx";
import { firstLaunchWords, protectionWords, type WordsRef } from "./words.ts";

const IT = '[data-testid="protection-it"]';
const fw = (key: FirstLaunchKey): WordsRef => firstLaunchWords(key);
const pw = (key: ProtectionKey): WordsRef => protectionWords(key);
const bw = (key: BlocklistKey): WordsRef => blocklistWords(key);
const COPY = fw("Copy details for IT");
const COPIED = fw("Message copied");
const required = (protection: Protection, press?: string[]): RoomScene => ({
  protection,
  over: { required: true },
  ...(press === undefined ? {} : { press }),
});
/** A firm's policy, active: `sandbox.allow_unprotected_auto` off (firmPolicy, blocked.ts), which the note and
 * Settings need before they offer the button. */
const FIRM: Partial<ProtectionView> = { unprotected_auto_allowed: false };
const firmNote = (protection: Protection): RoomScene => ({ ...note(protection), over: FIRM });
const unsupported = {
  not_protected: { cause: { unsupported: { why: "windows_arm" } } },
} as Protection;

/** The firm-only lines' entries, each id written as it is. */
export const IT_ENTRIES: Entry[] = [
  room("IT-01", COPY, "room/np-note", firmNote(P.policy)),
  room("IT-02", COPY, "room/protection-required", required(P.policy)),
  settings("IT-03", COPY, held(P.policy, FIRM)),
  blocklistRoom("IT-04", COPY, "no-override"),
  blocklistRoom("IT-05", COPY, "override-off"),
  room("IT-06", COPIED, "room/np-note", { protection: P.policy, over: FIRM, press: [LINE, IT] }),
  // the note's words: its lead, then its facts
  room(
    "IT-07",
    pw("Clauseo's Safe workspace is blocked on this computer."),
    "room/np-note",
    firmNote(P.policy),
  ),
  room(
    "IT-08",
    pw("The firm requires Clauseo's Safe workspace, and this computer can't provide it."),
    "room/protection-required",
    required(unsupported),
  ),
  blocklistRoom(
    "IT-09",
    bw(
      "Clauseo blocked a site on its unsafe-site list, and the firm's policy has turned off allowing it.",
    ),
    "no-override",
  ),
  room("IT-10", fw("Code: {code}"), "room/protection-required", required(P.policy)),
  room("IT-11", fw("Reason: {reason}"), "room/np-note", firmNote(P.policy)),
  blocklistRoom("IT-12", fw("Site: {site}"), "no-override"),
  room("IT-13", fw("Clauseo version: {version}"), "room/np-note", firmNote(P.policy)),
  room("IT-14", fw("Clauseo for firm IT: {url}"), "room/np-note", firmNote(P.policy)),
];
