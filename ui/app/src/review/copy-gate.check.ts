// The release gate, run by `pnpm -C ui/app copy:check` (vite.review.config.ts), never by `pnpm test` or CI: it fails
// while any registered string is not approved in copy-approvals.json. Strings whose words have no Hindi yet pass on
// an approval of their English with "hi": null, and are listed in one line, hi: later, so they stay tracked.
import { expect, test } from "vitest";
import { APPROVALS } from "./approvals.ts";
import { currentOf } from "./entry.ts";
import { REGISTRY } from "./registry.tsx";
import { duplicateIds, laterLine, malformedIds, pending, staleIds } from "./status.ts";

test("every registered string has a well-formed id of its own", () => {
  expect(duplicateIds(REGISTRY)).toEqual([]);
  expect(malformedIds(REGISTRY)).toEqual([]);
});

test("every registered string is approved in copy-approvals.json, as its words file has it now", () => {
  expect(REGISTRY.length).toBeGreaterThan(0);
  expect(pending(REGISTRY.map(currentOf), APPROVALS)).toEqual([]);
});

test("copy-approvals.json approves no id the registry no longer has", () => {
  expect(staleIds(REGISTRY, APPROVALS)).toEqual([]);
});

test("strings whose Hindi comes later are listed", () => {
  console.log(laterLine(REGISTRY.map(currentOf)));
});
