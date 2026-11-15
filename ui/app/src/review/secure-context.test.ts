import { expect, test } from "vitest";
import { uuidV4 } from "./secure-context.ts";

test("the stand-in for crypto.randomUUID makes version 4 UUIDs", () => {
  const ones = uuidV4((b) => b.fill(0xff));
  expect(ones).toBe("ffffffff-ffff-4fff-bfff-ffffffffffff");
  const zeros = uuidV4((b) => b.fill(0));
  expect(zeros).toBe("00000000-0000-4000-8000-000000000000");
  const random = uuidV4((b) => crypto.getRandomValues(b));
  expect(random).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
