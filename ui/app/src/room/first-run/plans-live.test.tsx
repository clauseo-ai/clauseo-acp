// Set up the other plan from the composer, then ask with it, without a restart: a room opened with only Claude set up
// still offers both plans on a new conversation (PK); choosing ChatGPT puts its one act, "Set up ChatGPT", where the
// question will go, with "Use Claude instead" beside it. Pressed, it starts ChatGPT's sign-in through the setup
// store's queue; once ChatGPT's install is ready and signed in, and the host's settings_changed brings its agents
// entry, the gate gives way to the field and ChatGPT's question starts a thread on codex-acp. The room is never
// remounted.
// @vitest-environment happy-dom
import { act } from "react";
import { expect, test } from "vitest";
import { DEFAULT_UI } from "../display-store.ts";
import {
  CLAUDE,
  CODEX,
  READY,
  SIGNED_IN,
  button,
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

const QUESTION = "What does clause nine say about notice?";
const field = () => byId<HTMLTextAreaElement>("composer") as HTMLTextAreaElement;
const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

test("ChatGPT set up from the composer while the room is open: its question goes to codex-acp", async () => {
  const listed = [CLAUDE];
  const t = await mountFirstRun({
    agents: listed,
    settingsPart: true,
    first: [install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN), signIn(CODEX, { signed_out: {} })],
  });
  // the room's top element, which a remount would replace
  const room = document.body.lastElementChild?.firstElementChild ?? null;
  expect(room).not.toBeNull();
  // both plans, on Claude, the one signed in
  expect(planButton("Claude")?.getAttribute("aria-pressed")).toBe("true");
  expect(planButton("ChatGPT")).not.toBeNull();
  expect(byId("signin")).toBeNull();

  // ChatGPT chosen: its one act where the question goes, and Claude as the way round it
  await press(planButton("ChatGPT"));
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  expect(text()).toContain("ChatGPT isn't set up on this computer.");
  expect(byId("signin-set-up")?.textContent).toBe("Set up ChatGPT");
  expect(button("Use Claude instead")).not.toBeNull();
  await press(byId("signin-set-up"));
  expect(t.setups()).toContainEqual({ start_sign_in: { agent: CODEX } });

  // ChatGPT's install reaches Ready and it signs in; setup writes agents.codex-acp, and the host says settings_changed
  await play(
    t,
    install(CODEX, { downloading: { received: 1, total: 200e6 } }),
    install(CODEX, READY),
  );
  listed.push(CODEX);
  await play(t, signIn(CODEX, SIGNED_IN));
  await act(async () => t.settingsChanged({ ui: DEFAULT_UI, locked: [] }));
  await settle();

  // the gate gave way to the field, ChatGPT still chosen
  expect(byId("signin-set-up")).toBeNull();
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
  expect(JSON.stringify(sent(t, "send_prompt"))).toContain(QUESTION);
  // the same room throughout
  expect(document.body.lastElementChild?.firstElementChild).toBe(room);
});
