// UD1, Done-when 1: the composer sends nothing to an agent that is not signed in; a draft typed before sign-in is sent
// only on a press after SignedIn; with no word from the host (a host before R20's desktop half), nothing is held back,
// so a returning lawyer whose plan is signed in sends without waiting on the first run.
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import {
  CLAUDE,
  READY,
  SIGNED_IN,
  byId,
  enter,
  install,
  mountFirstRun,
  play,
  press,
  sent,
  settle,
  signIn,
  typeInto,
} from "./rig.tsx";

const QUESTION = "What does clause nine say about notice?";
const field = () => byId<HTMLTextAreaElement>("composer") as HTMLTextAreaElement;
const asked = (t: Awaited<ReturnType<typeof mountFirstRun>>) => [
  ...sent(t, "create_thread"),
  ...sent(t, "send_prompt"),
];

test("the composer sends nothing to an agent that is not signed in: no Ask, and Enter and a submit send nothing", async () => {
  const t = await mountFirstRun({
    first: [install(CLAUDE, READY), signIn(CLAUDE, { signed_out: {} })],
  });
  expect(byId("signin")).not.toBeNull();
  expect(byId("send")).toBeNull();
  expect(field().hidden).toBe(true);
  await typeInto(field(), QUESTION);
  await enter(field());
  await act(async () => field().form?.requestSubmit());
  await settle();
  expect(asked(t)).toEqual([]);
});

test("a draft typed before sign-in stays a draft, and goes only with a press after SignedIn", async () => {
  // the host hasn't said anything of the sign-in yet: the field takes words
  const t = await mountFirstRun({ first: [install(CLAUDE, READY)] });
  await typeInto(field(), QUESTION);
  await play(t, signIn(CLAUDE, { signed_out: {} }));
  // the gate stands in the field's row; the words wait under it
  expect(byId("signin")).not.toBeNull();
  expect(field().value).toBe(QUESTION);
  await enter(field());
  expect(asked(t)).toEqual([]);
  await play(t, signIn(CLAUDE, { waiting: { since: 1, nudge: false } }), signIn(CLAUDE, SIGNED_IN));
  // signed in: the field is back with the words, and nothing went by itself
  expect(byId("signin")).toBeNull();
  expect(field().hidden).toBe(false);
  expect(field().value).toBe(QUESTION);
  expect(asked(t)).toEqual([]);
  await press(byId("send"));
  expect(sent(t, "create_thread")).toHaveLength(1);
  expect(sent(t, "create_thread")[0]?.agent).toBe(CLAUDE);
  expect(JSON.stringify(sent(t, "send_prompt"))).toContain(QUESTION);
});

test("with no word from the host (a desktop before R20's setup wire), the composer holds nothing back", async () => {
  const t = await mountFirstRun({ first: [] });
  expect(byId("signin")).toBeNull();
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread")).toHaveLength(1);
});

test("a returning lawyer signed in at the first word sees no gate and sends", async () => {
  const t = await mountFirstRun({ first: [install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN)] });
  expect(byId("signin")).toBeNull();
  // signed in before this window: no signed-in line to announce it
  expect(byId("signin-done")).toBeNull();
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread")).toHaveLength(1);
});
