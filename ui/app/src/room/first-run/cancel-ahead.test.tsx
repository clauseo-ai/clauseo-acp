// A plan installed and waiting behind the other plan's sign-in (one sign-in at a time). The plan ahead's own Cancel,
// on its own line, ends that sign-in, and the waiting plan's sign-in then goes with no further press: the store sends
// its start_sign_in when it held the wait, and desktop starts it itself when desktop held it (#1005). The waiting line
// offers "Cancel {name}'s sign-in" (UC-294) only while the plan ahead shows no line with its own Cancel: a device
// code or a switch of accounts, which the first screen's and the composer's lines don't show.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import { setupStoreOf, type SetupSource } from "../../store/setup.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import type { FakeTransport } from "../../transport/fake-transport.ts";
import type { SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  CODEX,
  READY,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  setups,
  signIn,
} from "./rig.tsx";

const storeOf = (t: FakeTransport) => setupStoreOf(t as unknown as HostTransport & SetupSource);
const BROWSER: SignIn = { waiting: { since: AT, nudge: false } };
const DEVICE: SignIn = { device_code: { url: "https://x", code: "ABCD-1234", expires_at: AT } };
const row = (agent: string) =>
  (allById("plan-row") as HTMLElement[]).find((r) => r.dataset["agent"] === agent) ?? null;
const inRow = (agent: string, id: string) =>
  row(agent)?.querySelector<HTMLElement>(`[data-testid="${id}"]`) ?? null;
/** The composer's other line, compact under the first. */
const also = () => document.querySelector<HTMLElement>(".room-entry .working-also");

test("first screen, ChatGPT waiting in the store behind Claude's browser sign-in: no second Cancel for Claude on ChatGPT's row; Claude's own Cancel sends cancel_sign_in, and ChatGPT's start_sign_in goes with no further press", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, BROWSER),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  storeOf(t).signInTo(CODEX);
  await play(t);
  expect(storeOf(t).state().queued).toBe(CODEX);
  expect(row(CODEX)?.textContent).toContain(
    "Your browser opens for ChatGPT after Claude's sign-in.",
  );
  expect(byId("signin-foot-cancel-ahead")).toBeNull();
  expect(inRow(CODEX, "signin-foot-cancel")?.textContent).toBe("Cancel");
  await press(inRow(CLAUDE, "signin-foot-cancel"));
  await play(t);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  expect(setups(t)).toEqual([
    { cancel_sign_in: { agent: CLAUDE } },
    { start_sign_in: { agent: CODEX } },
  ]);
});

test("first screen, desktop holding ChatGPT's sign-in: Claude's own Cancel sends cancel_sign_in alone, and desktop starts ChatGPT's (its row follows)", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, BROWSER),
      install(CODEX, READY),
      signIn(CODEX, "queued"),
    ],
  });
  expect(byId("signin-foot-cancel-ahead")).toBeNull();
  await press(inRow(CLAUDE, "signin-foot-cancel"));
  await play(t, signIn(CLAUDE, { signed_out: {} }), signIn(CODEX, BROWSER));
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CLAUDE } }]);
  expect(row(CODEX)?.textContent).toContain("ChatGPT · Finish signing in with your browser.");
});

test("first screen, ChatGPT's device code ahead (no line of its own here): Claude's row offers Cancel ChatGPT's sign-in, and Claude's sign-in goes once ChatGPT's ends", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, DEVICE),
    ],
  });
  storeOf(t).signInTo(CLAUDE);
  await play(t);
  const ahead = inRow(CLAUDE, "signin-foot-cancel-ahead");
  expect(ahead?.textContent).toBe("Cancel ChatGPT's sign-in");
  expect(inRow(CLAUDE, "signin-foot-cancel")?.textContent).toBe("Cancel");
  await press(ahead);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(setups(t)).toEqual([
    { cancel_sign_in: { agent: CODEX } },
    { start_sign_in: { agent: CLAUDE } },
  ]);
});

test("no press while the waiting plan is still downloading", async () => {
  await mountFirstRun({
    matters: [],
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, DEVICE),
      install(CODEX, { downloading: { received: 1, total: 2 } }),
      signIn(CODEX, "queued"),
    ],
  });
  expect(byId("signin-foot-cancel-ahead")).toBeNull();
});

test("a matter's gate, Claude waiting behind ChatGPT's browser sign-in: ChatGPT's line in the composer has its own Cancel and the gate none for it; that Cancel starts Claude's sign-in once ChatGPT's ends", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { failed: { code: "signin.link_expired", at: AT } }),
      install(CODEX, READY),
      signIn(CODEX, BROWSER),
    ],
  });
  await press("Try again");
  expect(setups(t)).toEqual([]);
  expect(byId("signin-cancel-ahead")).toBeNull();
  expect(byId("signin-cancel")).not.toBeNull();
  // Claude's install has no line, so ChatGPT's is the composer's one working line
  const line = document.querySelector<HTMLElement>(".room-entry .working");
  expect(line?.textContent).toContain("ChatGPT · Finish signing in with your browser.");
  const other = line?.querySelector<HTMLElement>('[data-testid="signin-foot-cancel"]') ?? null;
  expect(other?.textContent).toBe("Cancel");
  await press(other);
  await play(t);
  expect(setups(t)).toEqual([{ cancel_sign_in: { agent: CODEX } }]);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(setups(t)).toEqual([
    { cancel_sign_in: { agent: CODEX } },
    { start_sign_in: { agent: CLAUDE } },
  ]);
});

test("a matter's gate, Claude waiting behind ChatGPT's device code: no line for ChatGPT in the composer, so the gate offers Cancel ChatGPT's sign-in, then Claude's goes by itself", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { failed: { code: "signin.link_expired", at: AT } }),
      install(CODEX, READY),
      signIn(CODEX, DEVICE),
    ],
  });
  await press("Try again");
  expect(document.querySelector(".room-entry .working")).toBeNull();
  expect(also()).toBeNull();
  const ahead = byId("signin-cancel-ahead");
  expect(ahead?.textContent).toBe("Cancel ChatGPT's sign-in");
  await press(ahead);
  await play(t, signIn(CODEX, { signed_out: {} }));
  expect(setups(t)).toEqual([
    { cancel_sign_in: { agent: CODEX } },
    { start_sign_in: { agent: CLAUDE } },
  ]);
});
