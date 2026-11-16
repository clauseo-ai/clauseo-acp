// @vitest-environment happy-dom
// Two activity rows in plain words, through the real room over FakeTransport: a skill reads as a guide ("Read
// Clauseo's drafting guide.", or "Read a guide."), never by its internal `ns:name`, however the agent loads it; and a
// task that gave no words of its own but a title, or text it shows, is named by them ("Ran the checks.") in place of
// "Ran a task.", which stays where there is nothing to name it by. In English and in Hindi.
import { act } from "react";
import { steps } from "@clauseo/components/lib/words-steps.ts";
import { stepLive } from "@clauseo/components/lib/words-steps-live.ts";
import { afterEach, expect, test } from "vitest";
import { agentText, header, reset, toolCall, userMessage } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const ASKED = userMessage(5, 1, "Draft a reply to the bank's notice.");
const call = (status: ToolCallStatus, over: Partial<ToolCallEntry>): ViewEntry => ({
  tool_call: { ...toolCall(7, 290, status).tool_call!, title: "", ...over },
});
/** Claude's Skill tool, as claude-agent-acp titles it. */
const skill = (name: string, status: ToolCallStatus = "completed") =>
  call(status, {
    kind: "other",
    title: `Load skill: ${name}`,
    tool_name: "Skill",
    raw_input: { inline: JSON.stringify({ skill: name }) },
  });
const shown = () => document.body.textContent ?? "";

/** The settled group, opened to its rows: the group's text. */
async function rowOf(c: ViewEntry, language: "en" | "hi" = "en"): Promise<string> {
  h = await mountRoom({ display: { language } });
  h.deliver([reset([ASKED, c, agentText(8, 1, "The reply is drafted.")])]);
  await h.settle();
  await act(async () => h!.one("step-group")!.querySelector<HTMLButtonElement>("button")!.click());
  await h.settle();
  return h.one("step-group")!.textContent ?? "";
}

test.each([
  ["clauseo:legal-drafting", "Clauseo's drafting guide"],
  ["clauseo:legal-research", "Clauseo's research guide"],
  ["clauseo:matter-brief", "Clauseo's matter brief guide"],
  ["clauseo:agent-browser", "Clauseo's browser guide"],
] as const)(
  "a skill Clauseo ships, %s, reads as its guide; its internal name never shows",
  async (name, guide) => {
    expect(await rowOf(skill(name))).toContain(`Read ${guide}.`);
    expect(shown()).not.toContain(name);
    expect(shown()).not.toContain(name.split(":")[1]!);
    expect(shown()).not.toContain("Load skill");
  },
);

test("a skill Clauseo doesn't ship reads as a guide, done or tried, and its ns:name never shows", async () => {
  expect(await rowOf(skill("firm:house-style"))).toContain(steps("en", "Read a guide."));
  expect(shown()).not.toContain("house-style");
  await h!.unmount();
  expect(await rowOf(skill("firm:house-style", "failed"))).toContain(
    steps("en", "Tried to read a guide."),
  );
  expect(shown()).not.toContain("firm:");
});

test("a skill named only in its title, or only by the tool's name, reads as a guide too", async () => {
  expect(
    await rowOf(call("completed", { kind: "other", title: "Load skill: clauseo:legal-drafting" })),
  ).toContain("Read Clauseo's drafting guide.");
  await h!.unmount();
  expect(await rowOf(call("completed", { kind: "other", title: "Load skill" }))).toContain(
    "Read a guide.",
  );
  expect(shown()).not.toContain("Load skill");
});

test("ChatGPT's read of a skill's SKILL.md by a command reads as the guide", async () => {
  const c = call("completed", {
    kind: "execute",
    title: "sed -n '1,200p' /Users/x/Library/Clauseo/skills/legal-research/SKILL.md",
    raw_input: {
      inline: JSON.stringify({
        command: [
          "/bin/zsh",
          "-lc",
          "sed -n '1,200p' /Users/x/Library/Clauseo/skills/legal-research/SKILL.md",
        ],
      }),
    },
  });
  expect(await rowOf(c)).toContain("Read Clauseo's research guide.");
  expect(shown()).not.toContain("SKILL.md");
});

test("a skill being read: the working line says the guide", async () => {
  h = await mountRoom();
  h.deliver([reset([ASKED, skill("clauseo:legal-drafting", "in_progress")], header("running"))]);
  await h.settle();
  expect(h.one("working")!.textContent).toContain("Reading Clauseo's drafting guide.");
  expect(shown()).not.toContain("legal-drafting");
});

test("a task with a title in words is named by it, its Run dropped, in sentence case; tried as well", async () => {
  const titled = (status: ToolCallStatus) =>
    call(status, { kind: "execute", title: "Run the checks" });
  expect(await rowOf(titled("completed"))).toContain("Ran the checks.");
  await h!.unmount();
  expect(await rowOf(titled("failed"))).toContain("Tried to run the checks.");
  expect(shown()).not.toContain(steps("en", "Ran a task."));
});

test("a task with only the text it shows (Claude's description) is named by its first line", async () => {
  const c = call("completed", {
    kind: "execute",
    title: "Terminal",
    content: [{ text: { text: { inline: "Convert the MSA to Markdown\n" } } }],
  });
  expect(await rowOf(c)).toContain("Ran convert the MSA to Markdown.");
});

test("a task with nothing to name it by stays Ran a task.", async () => {
  expect(await rowOf(call("completed", { kind: "execute", title: "" }))).toContain(
    steps("en", "Ran a task."),
  );
  await h!.unmount();
  expect(await rowOf(call("completed", { kind: "execute", title: "Terminal" }))).toContain(
    steps("en", "Ran a task."),
  );
});

test("in Hindi: the guide, a guide and the named task in Hindi's words, the internal name never shown", async () => {
  const guide = stepLive("hi", "Clauseo's drafting guide");
  expect(await rowOf(skill("clauseo:legal-drafting"), "hi")).toContain(
    steps("hi", "Read {name}.", { name: guide }),
  );
  expect(shown()).not.toContain("legal-drafting");
  await h!.unmount();
  expect(await rowOf(skill("firm:house-style"), "hi")).toContain(steps("hi", "Read a guide."));
  expect(shown()).not.toContain("house-style");
  await h!.unmount();
  expect(
    await rowOf(call("completed", { kind: "execute", title: "Run the checks" }), "hi"),
  ).toContain(steps("hi", "Ran {description}.", { description: "the checks" }));
  expect(steps("hi", "Read a guide.")).not.toBe("Read a guide.");
});
