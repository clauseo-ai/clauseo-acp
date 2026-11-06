// Switching accounts (0048, #493): Clauseo is still stopping a login it may no longer use and retries by itself. The
// gate says so with nothing to press, and a stuck switch adds the way out. A setup command refused with
// command.conflict meanwhile says the switch isn't done, and the typed key stays for the retry; any other refusal says
// its code's own sentence. Nothing is held with a line: the gate stands in the field's row, so nothing is sent.
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import type { HostError, SignIn } from "../../transport/protocol.ts";
import {
  AT,
  CLAUDE,
  READY,
  byId,
  enter,
  info,
  install,
  mountFirstRun,
  play,
  press,
  sent,
  settle,
  setups,
  signIn,
  text,
  typeInto,
} from "./rig.tsx";

const SWITCHING: SignIn = { switching: { since: AT, stuck: false } };
const STATUS = "Switching accounts.";
const LINE =
  "Clauseo is still stopping the old login. It keeps trying by itself, so there's nothing you need to do.";
const QUIT = "If this goes on, quit Clauseo and open it again.";
const CONFLICT = "Clauseo is still switching accounts. Try again in a moment.";
const KEY = "sk-ant-api03-typed";

const gate = () => byId("signin-state");
const refusal = (code: string): HostError => ({
  code: code as never,
  message: "refused",
  retryable: true,
});

test("switching: the status and the line, with no button and no field", async () => {
  await mountFirstRun({ first: [info(), install(CLAUDE, READY), signIn(CLAUDE, SWITCHING)] });
  expect(gate()?.dataset["state"]).toBe("switching");
  expect(gate()?.textContent).toBe(`${STATUS} ${LINE}`);
  expect(gate()?.querySelector('[role="status"]')?.textContent).toContain(STATUS);
  expect(byId("signin")?.querySelectorAll("button, input")).toHaveLength(0);
  expect(text()).not.toContain(QUIT);
  expect(text()).not.toContain("Sign in to Claude");
});

test("switching, stuck: the quit line follows", async () => {
  await mountFirstRun({
    first: [
      info(),
      install(CLAUDE, READY),
      signIn(CLAUDE, { switching: { since: AT, stuck: true } }),
    ],
  });
  expect(gate()?.textContent).toBe(`${STATUS} ${LINE} ${QUIT}`);
  expect(byId("signin")?.querySelectorAll("button, input")).toHaveLength(0);
});

/** The key panel open while signed out, then the switch begins, then the key goes and desktop refuses it with `code`. */
async function keyRefused(code: string, switching: boolean) {
  const t = await mountFirstRun({
    first: [info(), install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
    command: (c) => {
      if (c.set_agent_key !== undefined) throw refusal(code);
    },
  });
  await press("Use an API key or my firm's account");
  if (switching) await play(t, signIn(CLAUDE, SWITCHING));
  const key = byId<HTMLInputElement>("signin-key") as HTMLInputElement;
  await typeInto(key, KEY);
  await press("Use this key");
  await settle();
  expect(setups(t)).toEqual([{ set_agent_key: { agent: CLAUDE, key: KEY } }]);
  return t;
}

test("a command.conflict while switching says the switch isn't done, and the typed key stays for the retry", async () => {
  const t = await keyRefused("command.conflict", true);
  expect(byId("signin-refused")?.textContent).toBe(CONFLICT);
  expect(byId("signin-refused")?.getAttribute("role")).toBe("status");
  expect(byId<HTMLInputElement>("signin-key")?.value).toBe(KEY);
  expect(byId("signin-key")?.parentElement?.querySelector("button:disabled")).toBeNull();
  // the next sign-in state answers the refusal
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  expect(byId("signin-refused")).toBeNull();
});

test("a command.conflict when not switching says the generic conflict line", async () => {
  await keyRefused("command.conflict", false);
  expect(byId("signin-refused")?.textContent).toBe(
    "That request clashes with one already made, so nothing changed.",
  );
  expect(byId<HTMLInputElement>("signin-key")?.value).toBe(KEY);
});

test("another refusal says its code's sentence, and a press clears it", async () => {
  const t = await mountFirstRun({
    first: [info(), install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
    command: (c) => {
      if (c.start_sign_in !== undefined) throw refusal("signin.key_in_use");
    },
  });
  await press("Sign in");
  await settle();
  expect(byId("signin-refused")?.textContent).toBe(
    "Clauseo is using your API key. To sign in with your plan instead, open Settings, then Plans, and remove the key.",
  );
  // opening the panel sends nothing, so the refusal stands; the next command answers it
  await press("Use an API key or my firm's account");
  expect(byId("signin-refused")).not.toBeNull();
  await typeInto(byId<HTMLInputElement>("signin-key") as HTMLInputElement, KEY);
  await press("Use this key");
  await settle();
  expect(setups(t)).toHaveLength(2);
  expect(byId("signin-refused")).toBeNull();
});

test("Send waits while desktop answers a key: a second press sends nothing more, and the field clears once taken", async () => {
  let answer: () => void = () => {};
  const t = await mountFirstRun({
    first: [info(), install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
    command: () => new Promise<void>((done) => (answer = done)),
  });
  await press("Use an API key or my firm's account");
  const key = byId<HTMLInputElement>("signin-key") as HTMLInputElement;
  await typeInto(key, KEY);
  await press("Use this key");
  await enter(key);
  expect(setups(t)).toHaveLength(1);
  expect(key.value).toBe(KEY);
  await act(async () => answer());
  await settle();
  expect(byId<HTMLInputElement>("signin-key")?.value).toBe("");
});

test("no line holds a question while switching: the gate stands in the field's row and nothing is sent", async () => {
  const t = await mountFirstRun({
    first: [info(), install(CLAUDE, READY), signIn(CLAUDE, SWITCHING)],
  });
  const field = byId<HTMLTextAreaElement>("composer") as HTMLTextAreaElement;
  expect(byId("send")).toBeNull();
  expect(field.hidden).toBe(true);
  await typeInto(field, "What does clause nine say about notice?");
  await enter(field);
  await settle();
  expect([...sent(t, "create_thread"), ...sent(t, "send_prompt")]).toEqual([]);
  expect(text()).not.toMatch(/waits until/);
});
