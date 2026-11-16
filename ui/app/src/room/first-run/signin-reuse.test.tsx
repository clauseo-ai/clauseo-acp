// UD1, "Use it" on a login that doesn't work here (FoundLogin reuse_unusable, S18's run-time fallback): the card says
// why above Sign in, with Claude's key offer; the next check's reuse_off, and a plain sign-out, say nothing of it.
// @vitest-environment happy-dom
import { beforeAll, expect, test } from "vitest";
import type { FoundLogin, SignIn } from "../../transport/protocol.ts";
import {
  CLAUDE,
  READY,
  byId,
  info,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
  text,
} from "./rig.tsx";

// The first mount loads the room's lazy chunks (the first-run's read offer, the new-matter and locate-warning parts,
// the thread list, the protection note, the expired-sign-in card), which takes seconds on a busy machine. They load
// here, under the hook's own 30 s, so the first test's 5 s limit is not spent on them.
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

const UNUSABLE =
  "Clauseo couldn't use your Claude Code sign-in. It may have expired, so sign in here instead.";

test("Use it on a login that doesn't work here: the line says so above Sign in; a plain sign-out says nothing", async () => {
  const t = await at({ login_found: { from: "claude_code" } });
  await press(byId("login-use"));
  expect(setups(t)).toEqual([{ use_found_login: { agent: CLAUDE } }]);
  await play(t, signIn(CLAUDE, { signed_out: { found: "reuse_unusable" } }));
  expect(byId("signin-state")?.dataset["state"]).toBe("signed-out");
  expect(text()).toContain(UNUSABLE);
  expect(text()).not.toContain("Clauseo found your Claude account");
  await press("Sign in");
  expect(setups(t).at(-1)).toEqual({ start_sign_in: { agent: CLAUDE } });
  await press("Use an API key instead");
  expect(byId("signin-key")).not.toBeNull();
});

test.each<[string, { found?: FoundLogin }]>([
  ["the next check's reuse off", { found: "reuse_off" }],
  ["a plain sign-out", {}],
])("%s: no line about a sign-in that didn't work", async (_, signedOut) => {
  await at({ signed_out: signedOut });
  expect(text()).not.toContain(UNUSABLE);
  expect(text()).not.toContain("may have expired");
});
