import { expect, test } from "vitest";
import { lineStatus, onRunningPage } from "./running-lines.mjs";

const HI_BY = "app track (provisional; Hindi-speaking lawyer to review)";
const approvedEnglish = {
  en: "Archived. Read only.",
  status: "approved",
  at: "2026-11-04",
  by: "Rohan",
};
const hindiProvisional = {
  ...approvedEnglish,
  hi: "संग्रहीत। केवल पढ़ने के लिए।",
  hi_status: "provisional",
  hi_at: "2026-11-06",
  hi_by: HI_BY,
};

test("the page lists a line whose English is provisional or whose Hindi is, and no other", () => {
  expect(onRunningPage(hindiProvisional)).toBe(true);
  expect(onRunningPage({ ...hindiProvisional, status: "change" })).toBe(true);
  expect(
    onRunningPage({ en: "a", hi: null, status: "provisional", at: "2026-11-04", by: "app track" }),
  ).toBe(true);
  expect(onRunningPage({ ...hindiProvisional, status: "provisional", by: "app track" })).toBe(true);
  expect(onRunningPage({ ...approvedEnglish, hi: null, hi_status: "later" })).toBe(false);
  expect(onRunningPage({ ...approvedEnglish, hi: "संग्रहीत" })).toBe(false);
  expect(
    onRunningPage({ ...approvedEnglish, hi: null, hi_status: "later", status: "change" }),
  ).toBe(false);
});

test("an approved English with a provisional Hindi reads as approved, its Hindi marked provisional", () => {
  const { badge, html } = lineStatus(hindiProvisional);
  expect(badge).toBe("Hindi provisional");
  expect(html).toBe(
    `English approved 2026-11-04 by Rohan. <span class="running-hi">Hindi (provisional): <span lang="hi">संग्रहीत। केवल पढ़ने के लिए।</span>. Added 2026-11-06 by ${HI_BY}.</span>`,
  );
});

test("a provisional English says when and by whom it was added, and its Hindi as it stands", () => {
  const base = { en: "a", status: "provisional", at: "2026-11-04", by: "app track" };
  expect(lineStatus({ ...base, hi: null })).toEqual({
    badge: "Provisional",
    html: `Added 2026-11-04 by app track. <span class="running-hi">Hindi: later.</span>`,
  });
  expect(lineStatus({ ...base, hi: "<ब>" }).html).toBe(
    `Added 2026-11-04 by app track. <span class="running-hi">Hindi: <span lang="hi">&lt;ब&gt;</span>.</span>`,
  );
  expect(
    lineStatus({ ...base, hi: "ब", hi_status: "provisional", hi_at: "2026-11-06", hi_by: HI_BY })
      .html,
  ).toContain(`Hindi (provisional): <span lang="hi">ब</span>. Added 2026-11-06 by ${HI_BY}.`);
});
