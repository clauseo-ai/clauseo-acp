// Claude's browser sign-in note: under "Finish signing in with your browser.", since Claude's sign-in always opens
// claude.ai's own sign-in page, even when the lawyer is signed in there. On the first screen's Claude row and in the
// matter's gate, only for Claude and only in that state.
// @vitest-environment happy-dom
import { expect, test } from "vitest";
import type { SignIn } from "../../transport/protocol.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  allById,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  signIn,
  text,
} from "./rig.tsx";

const NOTE = "Claude may ask you to sign in again in your browser. Use your Claude account.";
const WAITING: SignIn = { waiting: { since: 0, nudge: false } };
const notes = () => allById("plan-note") as HTMLElement[];
const row = (agent: string) =>
  (allById("plan-row") as HTMLElement[]).find((r) => r.dataset["agent"] === agent) ?? null;

test("first screen: Claude waiting on the browser has the note under its row, and only then", async () => {
  const t = await mountFirstRun({
    matters: [],
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(text()).not.toContain(NOTE);
  await play(t, signIn(CLAUDE, WAITING));
  expect(row(CLAUDE)?.textContent).toContain("Finish signing in with your browser.");
  expect(notes().map((n) => n.textContent)).toEqual([NOTE]);
  expect(row(CLAUDE)?.contains(notes()[0]!)).toBe(true);
  // other states: none
  await play(t, signIn(CLAUDE, { failed: { code: "signin.cancelled", at: 0 } }));
  expect(text()).not.toContain(NOTE);
});

test("first screen: ChatGPT waiting on the browser has no note", async () => {
  await mountFirstRun({
    matters: [],
    first: [install(CODEX, READY), signIn(CODEX, WAITING)],
  });
  expect(row(CODEX)?.textContent).toContain("Finish signing in with your browser.");
  expect(text()).not.toContain(NOTE);
});

test("matter gate: Claude waiting on the browser has the note; signed out, it doesn't", async () => {
  const t = await mountFirstRun({
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(byId("signin")).not.toBeNull();
  expect(text()).not.toContain(NOTE);
  await play(t, signIn(CLAUDE, WAITING));
  expect(byId("signin")?.textContent).toContain(NOTE);
});

test("matter gate: ChatGPT waiting on the browser has no note", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      signIn(CLAUDE, { signed_out: {} }),
      install(CODEX, READY),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  await press("Sign in with ChatGPT");
  await play(t, signIn(CODEX, WAITING));
  expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
  expect(byId("signin")?.textContent).toContain("Finish signing in with your browser.");
  expect(text()).not.toContain(NOTE);
});
