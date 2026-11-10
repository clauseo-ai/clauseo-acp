// Install and sign in, then New conversation, without a restart: a room opened with only Claude set up shows no plan
// choice; once ChatGPT's install is ready and signed in, and the host's settings_changed brings its agents entry, New
// conversation offers Claude and ChatGPT, and ChatGPT's question starts a thread on codex-acp. The room is never
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
  byId,
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
const planButton = (name: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button[aria-pressed]")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;

test("ChatGPT set up while the room is open: New conversation offers it, and its question goes to codex-acp", async () => {
  const listed = [CLAUDE];
  const t = await mountFirstRun({
    agents: listed,
    settingsPart: true,
    first: [install(CLAUDE, READY), signIn(CLAUDE, SIGNED_IN), signIn(CODEX, { signed_out: {} })],
  });
  // the room's top element, which a remount would replace
  const room = document.body.lastElementChild?.firstElementChild ?? null;
  expect(room).not.toBeNull();
  expect(planButton("Claude")).toBeNull();
  expect(planButton("ChatGPT")).toBeNull();

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

  await press(byId("new-conversation"));
  expect(planButton("Claude")).not.toBeNull();
  expect(planButton("ChatGPT")).not.toBeNull();
  await press(planButton("ChatGPT"));
  expect(planButton("ChatGPT")?.getAttribute("aria-pressed")).toBe("true");
  await typeInto(field(), QUESTION);
  await press(byId("send"));
  expect(sent(t, "create_thread").map((c) => c.agent)).toEqual([CODEX]);
  expect(JSON.stringify(sent(t, "send_prompt"))).toContain(QUESTION);
  // the same room throughout
  expect(document.body.lastElementChild?.firstElementChild).toBe(room);
});
