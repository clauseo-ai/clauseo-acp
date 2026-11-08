// The scenarios in scenarios/ answer the window's first-read prompt (UD2) on any OS: each one's
// first `one_of` branch expects the very text ui/app/src/room/first-read/prompt.ts sends, and
// answers with flow.ts's FIRST_READ_ANSWER. A changed prompt fails here, not on the Windows runner,
// where the first read's launch would play the spec's turn instead.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { FIRST_READ_PROMPT } from "../../app/src/room/first-read/prompt.ts";
import { FILES_SCENARIO, FIRST_READ_ANSWER } from "./flow.ts";

const SCENARIOS = [
  FILES_SCENARIO,
  fileURLToPath(new URL("scenarios/word-one-docx.jsonl", import.meta.url)),
];

type Step = Record<string, any>;

for (const path of SCENARIOS) {
  test(`${path.split(/[\\/]/).pop()} answers the first-read prompt and touches no file`, () => {
    const steps: Step[] = readFileSync(path, "utf8")
      .split("\n")
      .filter((line) => line.trim() !== "")
      .map((line) => JSON.parse(line) as Step);
    const branches = steps.find((step) => step.one_of !== undefined)?.one_of as Step[][];
    expect(branches).toHaveLength(2);
    const [read = [], turn = []] = branches;
    expect(read[0]?.expect.params.prompt).toEqual([{ type: "text", text: FIRST_READ_PROMPT }]);
    expect(read.map((step) => step.update?.update?.content?.text).filter(Boolean)).toEqual([
      FIRST_READ_ANSWER,
    ]);
    expect(read.some((step) => Object.keys(step).some((key) => key.endsWith("_file")))).toBe(false);
    // the spec's own prompt: any text
    expect(turn[0]?.expect.method).toBe("session/prompt");
    expect(turn[0]?.expect.params.prompt).toBeUndefined();
  });
}
