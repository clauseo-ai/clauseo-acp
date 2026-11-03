import { readFileSync } from "node:fs";
import { expect, test } from "vitest";
import { HINDI_ANSWER } from "./hindi-answer.ts";

const SCENARIO = new URL(
  "../../../../crates/fake_agent/scenarios/claude-hindi-answer.jsonl",
  import.meta.url,
);

type Line = {
  update?: { update?: { sessionUpdate?: string; content?: { text?: string } } };
  respond?: { to?: string; result?: { modes?: { currentModeId?: string } } };
};
const lines = readFileSync(SCENARIO, "utf8")
  .split("\n")
  .filter((l) => l.trim() !== "")
  .map((l) => JSON.parse(l) as Line);
const chunks = lines.flatMap((l) =>
  l.update?.update?.sessionUpdate === "agent_message_chunk"
    ? [l.update.update.content?.text ?? ""]
    : [],
);
const bytes = (s: string) => new TextEncoder().encode(s).length;

test("fake_agent's claude-hindi-answer streams the recorded answer whole, byte for byte", () => {
  expect(chunks.join("")).toBe(HINDI_ANSWER);
  expect(HINDI_ANSWER.normalize("NFC")).toBe(HINDI_ANSWER);
  expect(HINDI_ANSWER).toMatch(/[\u0900-\u097F]/);
  expect(HINDI_ANSWER).toContain("₹2,00,00,000");
});

test("its chunks are 16 to 64 bytes, as an agent streams, and some split a conjunct after its virama", () => {
  for (const c of chunks) {
    expect(bytes(c), c).toBeGreaterThanOrEqual(16);
    expect(bytes(c), c).toBeLessThanOrEqual(64);
  }
  // the next chunk starts on the consonant a half form joins: a renderer that shapes per chunk shows श् then श्र
  const splits = chunks.slice(0, -1).filter((c) => c.endsWith("\u094D"));
  expect(splits.length).toBeGreaterThanOrEqual(5);
});

test("it answers session/new with modes, in default, as the host asks of a Claude agent", () => {
  const modes = lines.find((l) => l.respond?.to === "new")?.respond?.result?.modes;
  expect(modes?.currentModeId).toBe("default");
});
