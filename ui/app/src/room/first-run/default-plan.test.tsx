// D-014, D-043 in the room: a new conversation never opens on a plan that isn't signed in while another is. With only
// ChatGPT signed in, a new matter's and the Scratch matter's new conversation start on ChatGPT, with no Claude gate
// ("Sign-in was cancelled"); with both, the plan last asked with; a plan signing in mid-session moves the default with
// no remount; the lawyer's own pick stands.
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import { uuid } from "../../testing/stream-script.ts";
import type { MatterSummary, SignIn } from "../../transport/protocol.ts";
import { LAST_PLAN_KEY } from "../plan-default.ts";
import {
  A,
  AT,
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  byId,
  install,
  mountFirstRun,
  play,
  press,
  sent,
  settle,
  signIn,
  text,
  typeInto,
} from "./rig.tsx";

const SCRATCH: MatterSummary = { matter: uuid(9, 2), name: "Scratch", kind: "scratch" };
const CANCELLED: SignIn = { failed: { code: "signin.cancelled", at: AT } };
const QUESTION = "What does clause nine say about notice?";
const field = () => byId<HTMLTextAreaElement>("composer") as HTMLTextAreaElement;
const pressed = () =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed='true']")]
    .map((b) => b.textContent?.trim())
    .filter((t) => t === "Claude" || t === "ChatGPT");
const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

const ready = [install(CLAUDE, READY), install(CODEX, READY)];

test("only ChatGPT signed in: a new matter's new conversation starts on ChatGPT, with no Sign-in was cancelled", async () => {
  const t = await mountFirstRun({
    first: [...ready, signIn(CLAUDE, CANCELLED), signIn(CODEX, SIGNED_IN)],
  });
  expect(pressed()).toEqual(["ChatGPT"]);
  expect(byId("signin")).toBeNull();
  expect(text()).not.toContain("Sign-in was cancelled");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
  // the question's plan is the one last asked with, for the next new matter
  expect(localStorage.getItem(LAST_PLAN_KEY)).toBe(CODEX);
});

test("only ChatGPT signed in: the Scratch matter's new conversation starts on ChatGPT too", async () => {
  const t = await mountFirstRun({
    matters: [A, SCRATCH],
    first: [...ready, signIn(CLAUDE, CANCELLED), signIn(CODEX, SIGNED_IN)],
  });
  await act(async () => document.querySelector<HTMLElement>("[data-kind=scratch]")?.click());
  await settle();
  expect(pressed()).toEqual(["ChatGPT"]);
  expect(text()).not.toContain("Sign-in was cancelled");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread")).toEqual([{ matter: SCRATCH.matter, agent: CODEX }]);
});

test("both signed in: the plan last asked with wins; with none kept, Claude", async () => {
  localStorage.setItem(LAST_PLAN_KEY, CODEX);
  await mountFirstRun({ first: [...ready, signIn(CLAUDE, SIGNED_IN), signIn(CODEX, SIGNED_IN)] });
  expect(pressed()).toEqual(["ChatGPT"]);
});

test("both signed in and nothing kept: Claude, as before", async () => {
  await mountFirstRun({ first: [...ready, signIn(CLAUDE, SIGNED_IN), signIn(CODEX, SIGNED_IN)] });
  expect(pressed()).toEqual(["Claude"]);
});

test("a plan signing in mid-session moves the default, with no remount", async () => {
  const t = await mountFirstRun({
    first: [...ready, signIn(CLAUDE, { signed_out: {} }), signIn(CODEX, { signed_out: {} })],
  });
  // neither signed in: Claude, and its gate offers the sign-in
  expect(pressed()).toEqual(["Claude"]);
  expect(byId("signin")?.dataset["agent"]).toBe("claude");
  const composer = field();
  await play(t, signIn(CODEX, SIGNED_IN));
  expect(pressed()).toEqual(["ChatGPT"]);
  expect(byId("signin")).toBeNull();
  expect(field()).toBe(composer);
  // and back, when ChatGPT signs out and Claude signs in
  await play(t, signIn(CODEX, { signed_out: {} }), signIn(CLAUDE, SIGNED_IN));
  expect(pressed()).toEqual(["Claude"]);
});

test("the lawyer's pick stands over a later sign-in", async () => {
  const t = await mountFirstRun({
    first: [...ready, signIn(CLAUDE, { signed_out: {} }), signIn(CODEX, SIGNED_IN)],
  });
  await press(planButton("Claude"));
  expect(pressed()).toEqual(["Claude"]);
  await play(t, signIn(CLAUDE, SIGNED_IN), signIn(CODEX, SIGNED_IN));
  expect(pressed()).toEqual(["Claude"]);
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CLAUDE]);
});
