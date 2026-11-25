// #1350's review: agent.key_denied's line by the computer. Only the Mac's names its keychain; a platform the UI can't
// name (computerOf's undefined), like Windows and Linux, gets the line with no Mac sentence
// (U3-err-agent.key_denied.windows_linux).
import { expect, test } from "vitest";
import { KEY_DENIED_ELSEWHERE } from "@clauseo/components/lib/words-errors.ts";
import { CLAUDE, type SetupState } from "../../store/setup.ts";
import { computerOf, welcomeFoot, type FootActs } from "./welcome-foot.tsx";

const MAC =
  "Your Mac's keychain didn't let Clauseo read, save or remove the API key. Press Allow on Your keychain, then choose Always Allow.";

const setup = {
  install: { ready: { version: "0.1.0" } },
  signIn: { failed: { code: "agent.key_denied" as const, at: "2026-01-01T00:00:00Z" } },
};
const state = {
  keys: true,
  python: null,
  agents: { [CLAUDE]: setup },
  chosen: CLAUDE,
  picked: false,
  queued: null,
  pending: null,
  keyOpen: false,
  pasting: false,
  copied: null,
  counts: { on: false, locked: false, started: false },
  locked: [],
} as unknown as SetupState;
const nothing = () => {};
// every press does nothing; no keychain pointer, so the key's own line shows
const acts = new Proxy(
  {},
  { get: (_, key) => (key === "keychain" ? undefined : nothing) },
) as FootActs;
const ids = { line: "l", said: "s", act: "a", cancel: "c" } as never;

const said = (platform: string) =>
  JSON.stringify(
    welcomeFoot(
      "en",
      CLAUDE,
      "Claude",
      setup as never,
      state,
      true,
      acts,
      ids,
      computerOf(platform),
    ),
  );

test.each(["something_new", "windows", "linux"])(
  "key-denied on %s: the line with no Mac sentence",
  (platform) => {
    expect(said(platform)).toContain(KEY_DENIED_ELSEWHERE);
    expect(said(platform)).not.toContain("Your Mac's keychain");
  },
);

test("key-denied on the Mac: the Mac's line", () => {
  expect(said("mac")).toContain(MAC);
  expect(said("mac")).not.toContain(KEY_DENIED_ELSEWHERE);
});
