// UD1, the gate for a saved key the Mac's keychain didn't give (agent.key_unreadable, decision record 0048): the
// key's own words, never "Sign-in didn't finish.", and Try again, which reads the key again (desktop refuses the
// sign-in start with agent.key_unreadable and re-reads it). Kept apart from signin.test.tsx for its 600-line limit.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  READY,
  info,
  install,
  mountFirstRun,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

// The first mount loads the room's lazy chunks, which takes seconds on a busy machine: they load here, under the
// hook's own 30 s, so the test's 5 s limit is not spent on them (as signin.test.tsx does).
beforeAll(async () => {
  await Promise.all([
    import("../first-read/first-read-offer.tsx"),
    import("../new-matter.tsx"),
    import("../locate-warning.tsx"),
    import("../thread/list.tsx"),
    import("../protection/note-load.ts").then((m) => m.loadProtectionNote()),
    import("../signin-expired/expired-card.tsx"),
  ]);
}, 30_000);

/** The room with matter A open, Claude installed, and this sign-in state. */
const at = (state: SignIn) =>
  mountFirstRun({ first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, state)] });

test("key-unreadable: the saved key in its own words, never Sign-in didn't finish, and Try again", async () => {
  const t = await at({ failed: { code: "agent.key_unreadable", at: AT } });
  expect(text()).toContain("Clauseo couldn't check for a saved API key in time.");
  expect(text()).not.toContain("Sign-in didn't finish.");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});
