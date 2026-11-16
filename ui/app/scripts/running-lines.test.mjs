import { expect, test } from "vitest";
import {
  byArea,
  englishStatus,
  lineStatus,
  onEnglishPage,
  onRunningPage,
} from "./running-lines.mjs";

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

test("the English page lists every line whose English is not approved, whatever its Hindi", () => {
  expect(onEnglishPage({ ...approvedEnglish, hi: null, hi_status: "later" })).toBe(false);
  expect(onEnglishPage(hindiProvisional)).toBe(false);
  expect(onEnglishPage({ ...approvedEnglish, hi: null, status: "provisional" })).toBe(true);
  expect(
    onEnglishPage({ ...approvedEnglish, hi: null, hi_status: "later", status: "change" }),
  ).toBe(true);
});

test("the English page groups by the screen's area, by screen within it, and says when a line was added", () => {
  const line = (id, screen) => ({ id, entry: screen === undefined ? undefined : { screen } });
  const groups = byArea([
    line("A-01", "settings/tools"),
    line("A-02", "room/thread"),
    line("A-03", "room/composer"),
    line("A-04"),
    line("A-05", "room/thread"),
  ]);
  expect(groups.map((g) => [g.area, g.lines.map((l) => l.id)])).toEqual([
    ["not in the review registry", ["A-04"]],
    ["room", ["A-03", "A-02", "A-05"]],
    ["settings", ["A-01"]],
  ]);
  const base = { en: "a", hi: null, at: "2026-11-08", by: "app <track>" };
  expect(englishStatus({ ...base, status: "provisional" })).toEqual({
    badge: "Provisional",
    html: "Added 2026-11-08 by app &lt;track&gt;.",
  });
  expect(englishStatus({ ...base, status: "change" }).badge).toBe("Change asked for");
});
