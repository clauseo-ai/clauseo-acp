// @vitest-environment happy-dom
// The host's pick_not_used notice (notice-words.ts): the agent couldn't use the model or effort picked for an answer,
// so it ran with what the session holds. A quiet row at the top of that answer, never an error, in each form: the
// model and the effort, the model only, the effort only, neither; the agent by name. No agent known: the host's text.
import { afterEach, expect, test } from "vitest";
import { th } from "@clauseo/components/lib/words-thread.ts";
import { agentText, reset, step, userMessage, uuid } from "../../testing/stream-script.ts";
import type { NoticeKind, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, type Harness } from "./harness.ts";
import { noticeLine, type NoticeWords } from "./notice-words.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const w: NoticeWords = {
  lang: "en",
  th: (key, slots) => th("en", key, slots),
  t: (key) => key,
};
const CLAUDE = { kind: "claude" as const, name: "claude-acp" };
const CHATGPT = { kind: "codex" as const, name: "codex-acp" };
const kind = (model?: string, effort?: string): NoticeKind => ({
  pick_not_used: {
    ...(model === undefined ? {} : { model }),
    ...(effort === undefined ? {} : { effort }),
  },
});
const plain = (s: string | null) => s?.replace(/[\u2066-\u2069]/g, "") ?? null;

test.each([
  [
    "the model and the effort",
    "Opus 5.5",
    "Medium",
    "Claude couldn't use the model or effort you picked for this answer, so it used Opus 5.5 at Medium effort.",
  ],
  [
    "the model only",
    "Opus 5.5",
    undefined,
    "Claude couldn't use the model or effort you picked for this answer, so it used Opus 5.5.",
  ],
  [
    "the effort only",
    undefined,
    "Medium",
    "Claude couldn't use the model or effort you picked for this answer, so it used Medium effort.",
  ],
  [
    "neither",
    undefined,
    undefined,
    "Claude couldn't use the model or effort you picked for this answer, so it kept the settings it had.",
  ],
] as const)(
  "%s: one line naming the agent and what the answer ran with",
  (_, model, effort, words) => {
    expect(plain(noticeLine(kind(model, effort), w, CLAUDE))).toBe(words);
  },
);

test("ChatGPT by its name; no agent known gives null, so the host's text shows", () => {
  expect(plain(noticeLine(kind("6.2 Sol", "High"), w, CHATGPT))).toBe(
    "ChatGPT couldn't use the model or effort you picked for this answer, so it used 6.2 Sol at High effort.",
  );
  expect(noticeLine(kind("Opus 5.5"), w, undefined)).toBeNull();
});

test("in the thread: a quiet row at the top of the answer, never an alert", async () => {
  const notice: ViewEntry = {
    notice: {
      order: 6,
      event: uuid(6, 7),
      level: "info",
      text: "Clauseo couldn't switch to the model or effort you picked for this answer, so it used Opus 5.5.",
      kind: kind("Opus 5.5"),
      ts: 6000,
    },
  };
  h = await mountRoom();
  await h.deliverAwaited([
    reset([
      userMessage(5, 1, "What does clause nine say?"),
      notice,
      agentText(20, 1, "It says notice."),
      step(30),
    ]),
  ]);
  await h.settle();
  const text = plain(h.host.textContent ?? "") ?? "";
  expect(text).toContain(
    "Claude couldn't use the model or effort you picked for this answer, so it used Opus 5.5.",
  );
  expect(text).not.toContain("Clauseo couldn't switch");
  expect(h.host.querySelector('[role="alert"]')).toBeNull();
  // above the answer's words
  const at = (s: string) => text.indexOf(s);
  expect(at("couldn't use the model")).toBeLessThan(at("It says notice."));
});
