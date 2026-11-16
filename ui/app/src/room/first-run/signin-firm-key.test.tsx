// UD1, the gate for a firm endpoint with no key saved (agent.firm_key_needed, decision record 0048): its words alone,
// no button, never "Sign-in didn't finish.": signing in can't help, only saving the firm's key in Settings, Plans.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { SignIn } from "../../transport/protocol.ts";
import { AT, CLAUDE, READY, byId, info, install, mountFirstRun, signIn } from "./rig.tsx";

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

test("agent.firm_key_needed: its words only, with no button and never Sign-in didn't finish", async () => {
  await at({ failed: { code: "agent.firm_key_needed", at: AT } });
  const state = byId("signin-state")?.textContent ?? "";
  expect(state).toContain(
    "Claude can't start here until an API key for your firm's gateway is saved.",
  );
  expect(state).not.toContain("Sign-in didn't finish");
  expect(state).not.toContain("Try again");
  expect(byId("signin-state")?.querySelectorAll("button")).toHaveLength(0);
});
