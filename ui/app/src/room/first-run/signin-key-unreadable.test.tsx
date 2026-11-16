// UD1, the gate for a saved key the Mac's keychain didn't give (agent.key_unreadable, decision record 0048): the
// key's own words, never "Sign-in didn't finish.", and Try again, which reads the key again (desktop refuses the
// sign-in start with agent.key_unreadable and re-reads it). Kept apart from signin.test.tsx for its 600-line limit.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { HostError, SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  READY,
  byId,
  info,
  install,
  mountFirstRun,
  press,
  settle,
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

const refusal = (code: string): HostError => ({
  code: code as never,
  message: "refused",
  retryable: false,
});

/** The room with matter A open, Claude installed, and this sign-in state; `refuse` refuses every sign-in start. */
const at = (state: SignIn, refuse?: string) =>
  mountFirstRun({
    first: [info(true), install(CLAUDE, READY), signIn(CLAUDE, state)],
    command: (c) => {
      if (refuse !== undefined && c.start_sign_in !== undefined) throw refusal(refuse);
    },
  });
const KEY_WORDS = "Clauseo couldn't check for a saved API key in time.";
const UNREADABLE: SignIn = { failed: { code: "agent.key_unreadable", at: AT } };

test("key-unreadable: the saved key in its own words, never Sign-in didn't finish, and Try again", async () => {
  const t = await at({ failed: { code: "agent.key_unreadable", at: AT } });
  expect(text()).toContain("Clauseo couldn't check for a saved API key in time.");
  expect(text()).not.toContain("Sign-in didn't finish.");
  await press("Try again");
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
});

test("key-unreadable: the gate also says where the key can be changed, Settings then Plans", async () => {
  await at(UNREADABLE);
  expect(text()).toContain(
    `${KEY_WORDS} If this computer asks for permission, allow it, then try again.`,
  );
  expect(byId("signin-state")?.textContent).toContain("To change it, open Settings, then Plans.");
});

test("key-unreadable: Try again refused in the same code adds no second line under the gate", async () => {
  const t = await at(UNREADABLE, "agent.key_unreadable");
  await press("Try again");
  await settle();
  expect(setups(t)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-refused")).toBeNull();
  // the gate (its refused line included) says the key's words once
  expect(byId("signin-state")?.textContent?.split(KEY_WORDS)).toHaveLength(2);
});

test("key-unreadable: Try again refused in another code says that code's line under the gate", async () => {
  const other = await at(UNREADABLE, "signin.key_in_use");
  await press("Try again");
  await settle();
  expect(setups(other)).toEqual([{ start_sign_in: { agent: CLAUDE } }]);
  expect(byId("signin-refused")?.textContent).toContain("Clauseo is using your API key.");
});
