// A case-law step's cost on its row: the credits after the done or tried line, "nothing charged"
// for a refused call, nothing for a free call or a step not done, and no sum on a folded group.
import { describe, expect, test } from "vitest";
import { toolCall } from "../../testing/stream-script.ts";
import type { ToolCallEntry, ToolCallStatus } from "../../transport/protocol.ts";
import { chargeOf, doneOf, restOf, rowTextOf, summaryOf } from "./steps.ts";

let n = 0;
/** A case-law search call in `status`, with its charge when given. */
function call(status: ToolCallStatus, over: Partial<ToolCallEntry> = {}): ToolCallEntry {
  n += 1;
  return { ...toolCall(10 + n, 300 + n, status).tool_call!, kind: "other", title: "", ...over };
}

describe("a case-law step's cost", () => {
  const search = (status: ToolCallStatus, charge?: ToolCallEntry["charge"]) =>
    call(status, {
      title: "mcp__case-law__search_law",
      ...(charge === undefined ? {} : { charge }),
    });

  test("the credits follow the done line after a space, one and many apart", () => {
    const paid = search("completed", { credits: 40, refused: false });
    expect(chargeOf("en", paid)).toBe("· 40 credits");
    expect(rowTextOf("en", paid)).toBe(`${doneOf("en", paid)} · 40 credits`);
    expect(chargeOf("en", search("completed", { credits: 1, refused: false }))).toBe("· 1 credit");
  });

  test("a refused or failed call says nothing was charged, after its tried line too", () => {
    const refused = search("failed", { credits: 0, refused: true });
    expect(chargeOf("en", refused)).toBe("· nothing charged");
    expect(rowTextOf("en", refused)).toBe(`${doneOf("en", refused)} · nothing charged`);
    expect(chargeOf("en", search("completed", { credits: 0, refused: true }))).toBe(
      "· nothing charged",
    );
  });

  test("no charge, a free call, or a step not done or tried: the plain words", () => {
    const plain = search("completed");
    expect(chargeOf("en", plain)).toBeNull();
    expect(rowTextOf("en", plain)).toBe(doneOf("en", plain));
    expect(chargeOf("en", search("completed", { credits: 0, refused: false }))).toBeNull();
    for (const status of ["pending", "in_progress", "cancelled", "unknown"] as const)
      expect(chargeOf("en", search(status, { credits: 40, refused: false })), status).toBeNull();
  });

  test("a folded group never sums the cost", () => {
    const calls = [
      search("completed", { credits: 40, refused: false }),
      search("completed", { credits: 1, refused: false }),
    ];
    const line = restOf("en", calls);
    expect(line.line).not.toMatch(/credit/);
    expect(summaryOf("en", calls)).not.toMatch(/credit/);
  });

  test("the Hindi display shows the English until the Hindi comes", () => {
    expect(chargeOf("hi", search("completed", { credits: 40, refused: false }))).toBe(
      "· 40 credits",
    );
  });
});
