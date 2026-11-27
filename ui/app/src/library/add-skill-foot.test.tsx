// @vitest-environment happy-dom
// Add a skill's Add and Cancel stay reachable at the windows' least and default sizes (RC2, Windows): the action row
// is sticky at the foot of whatever scrolls the panel, on that scroller's ground; Settings' pane is that scroller; the
// place under 1000 px, a page the window scrolls, holds no scroller of its own to trap the row; and each new preview
// scrolls into its scroller, so its skills and Add come into sight wherever the panel starts.
import { settled } from "@clauseo/components/lib/late.ts";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, test, vi } from "vitest";
import { FakeTransport } from "../transport/fake-transport.ts";
import { AddSkillPanel } from "./add-skill.tsx";
import { FakeSkillLinks } from "./add-skill-fake.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const here = dirname(fileURLToPath(import.meta.url));
const read = (path: string) => readFileSync(join(here, path), "utf8");
/** The declarations of the first rule whose selector is exactly `selector`. */
const rule = (sheet: string, selector: string): string => {
  const at = sheet.search(new RegExp(`(^|\\n)\\s*${selector.replace(/[.[\]]/g, "\\$&")}\\s*\\{`));
  expect(at, `${selector} has a rule`).toBeGreaterThanOrEqual(0);
  const open = sheet.indexOf("{", at);
  return sheet.slice(open + 1, sheet.indexOf("}", open));
};

test("the action row sticks to its scroller's foot, on the scroller's ground", () => {
  const acts = rule(read("add-skill.css"), ".add-skill-acts");
  expect(acts).toMatch(/position:\s*sticky;/);
  expect(acts).toMatch(/bottom:\s*0;/);
  expect(acts).toMatch(/background:\s*var\(--add-skill-ground, var\(--surface\)\);/);
  // the place's side stands on the ground, so its row does too
  expect(rule(read("library.css"), ".lib-side")).toMatch(/--add-skill-ground:\s*var\(--ground\);/);
});

test("Settings' pane is the scroller, bounded by its grid row", () => {
  const sheet = read("../../../components/src/clauseo/settings-window.css");
  const pane = rule(sheet, ".set-pane");
  expect(pane).toMatch(/min-height:\s*0;/);
  expect(pane).toMatch(/overflow-y:\s*auto;/);
  expect(rule(sheet, ".set-win")).toMatch(/grid-template-rows:\s*minmax\(0, 1fr\);/);
});

test("under 1000 px the place holds no scroller of its own, so the row sticks to the window", () => {
  const sheet = read("library.css");
  const mid = sheet.slice(sheet.indexOf("@variant max-mid {"));
  expect(mid).toMatch(/\.lib-split,\s*\.lib-list,\s*\.lib-side\s*\{\s*overflow-y:\s*visible;/);
  expect(mid).toMatch(/\.lib-side\s*\{\s*max-height:\s*none;/);
  // after the narrow container's rule, which makes the stack a scroller, so it wins
  expect(sheet.indexOf("@variant max-mid {")).toBeGreaterThan(
    sheet.indexOf("@container library (max-width: 720px)"),
  );
});

test("each window's first load carries the sheet: Settings, the place, and the review", () => {
  const imports = '@import "../library/add-skill.css";';
  expect(read("../settings/settings.css")).toContain(imports);
  expect(read("../review/review.css")).toContain(imports);
  expect(read("library.css")).toContain('@import "./add-skill.css";');
});

test("a preview scrolls into view as far as it fits, with Add its last row", async () => {
  const links = new FakeSkillLinks();
  const transport = new FakeTransport({
    query: async (query) => {
      const answer = await links.query(query);
      if (answer === null) throw new Error("not scripted");
      return answer;
    },
    dispatch: async (envelope) => (await links.dispatch(envelope)) ?? "done",
    pickSkill: () => "/Users/r/Downloads/lease-review.zip",
  });
  const into = vi.spyOn(HTMLElement.prototype, "scrollIntoView").mockImplementation(() => {});
  const el = document.createElement("div");
  document.body.append(el);
  const root = createRoot(el);
  try {
    await act(async () =>
      root.render(
        <AddSkillPanel transport={transport} matter={null} scope0="all" onAdded={() => {}} />,
      ),
    );
    expect(into).not.toHaveBeenCalled();
    await act(async () => el.querySelector<HTMLElement>('[data-testid="add-skill-zip"]')!.click());
    for (let i = 0; i < 4; i += 1) await act(async () => settled());
    const preview = el.querySelector('[data-testid="add-skill-preview"]')!;
    expect(into).toHaveBeenCalledWith({ block: "nearest" });
    expect(into.mock.contexts.at(-1)).toBe(preview);
    expect(preview.lastElementChild?.className).toBe("add-skill-acts");
    expect(preview.lastElementChild?.querySelector('[data-testid="add-skill-add"]')).not.toBeNull();
  } finally {
    await act(async () => root.unmount());
    el.remove();
    into.mockRestore();
  }
});
