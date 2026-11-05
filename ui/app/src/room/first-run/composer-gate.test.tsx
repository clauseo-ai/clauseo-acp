// UD1, Done-when 1: the composer sends nothing to an agent that is not signed in; a draft typed before sign-in is sent
// only on a press after SignedIn; with no word from the host (a host before R20's desktop half), nothing is held back,
// so a returning lawyer whose plan is signed in sends without waiting on the first run.
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import {
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  button,
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

/** The composer's plan switch's button for a plan, as the lawyer presses it. */
const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

test("the plan switch's ChatGPT sticks with both plans listed and signed in, and the send goes to codex-acp", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      signIn(CLAUDE, SIGNED_IN),
      signIn(CODEX, SIGNED_IN),
    ],
  });
  await press(planButton("ChatGPT"));
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
});

test("with no word from the host (the desktop stub), the plan switch's ChatGPT sticks too", async () => {
  const t = await mountFirstRun({ first: [] });
  await press(planButton("ChatGPT"));
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
});

test("the gate checks the plan the send uses: ChatGPT picked in the switch while only Claude is signed in is held", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      signIn(CLAUDE, SIGNED_IN),
      signIn(CODEX, { signed_out: {} }),
    ],
  });
  expect(byId("signin")).toBeNull();
  await typeInto(field(), QUESTION);
  await press(planButton("ChatGPT"));
  expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
  await enter(field());
  await act(async () => field().form?.requestSubmit());
  await settle();
  expect(asked(t)).toEqual([]);
  // back to Claude, signed in: the gate goes and the question can go
  await press(planButton("Claude"));
  expect(byId("signin")).toBeNull();
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CLAUDE]);
});

test("Use ChatGPT instead takes the new conversation to ChatGPT once; the lawyer's later pick of Claude stands", async () => {
  const t = await mountFirstRun({
    first: [
      install(CLAUDE, READY),
      install(CODEX, READY),
      signIn(CLAUDE, { signed_out: {} }),
      signIn(CODEX, SIGNED_IN),
    ],
  });
  await press("Use ChatGPT instead");
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  expect(byId("signin")).toBeNull();
  await press(planButton("Claude"));
  await settle();
  expect(planButton("Claude")?.getAttribute("aria-pressed")).toBe("true");
  expect(byId("signin")?.dataset["agent"]).toBe("claude");
  expect(asked(t)).toEqual([]);
});

test.each<[string, string[]]>([
  ["no plan listed yet", []],
  ["only Claude listed", [CLAUDE]],
])(
  "on a first run (%s), Use ChatGPT instead switches the gate and the send to codex-acp at once",
  async (_, agents) => {
    const t = await mountFirstRun({
      agents,
      first: [
        install(CLAUDE, { downloading: { received: 1, total: 200e6 } }),
        signIn(CLAUDE, { signed_out: {} }),
        signIn(CODEX, { signed_out: {} }),
      ],
    });
    await press("Use ChatGPT instead");
    expect(t.setups()).toEqual([{ install_agent: { agent: CODEX } }]);
    expect(byId("signin")?.dataset["agent"]).toBe("chatgpt");
    expect(button("Use Claude instead")).not.toBeNull();
    expect(button("Use ChatGPT instead")).toBeNull();
    await play(t, install(CODEX, READY), signIn(CODEX, SIGNED_IN));
    expect(byId("signin")).toBeNull();
    await typeInto(field(), QUESTION);
    await press(byId("send"));
    expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
  },
);
