// The words of "Always for this matter" (decision record 0063): none is kept that nothing shows
// (UC-813, UC-815), in its copy file or in the record's words table.
import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const words = JSON.parse(read("../copy-approvals/UC-always-for-this-matter.json"));
const record = read("../../../docs/decisions/0063-always-for-this-matter.md");

test("no word is kept that nothing shows", () => {
  for (const gone of ["UC-813", "UC-815"]) {
    expect(Object.keys(words)).not.toContain(gone);
    expect(record).not.toMatch(new RegExp(`^\\| ${gone} \\|`, "m"));
  }
});
