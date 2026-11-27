// @vitest-environment happy-dom
// Add a skill in Skills and connectors (RC A1) in the real room over FakeTransport: the head's
// Add a skill on Skills opens the flow in the side panel, with the matter open last to choose;
// a link previews and Add sends add_skills, then the list is read again and the new row shows
// under "Added by you". Connectors has no Add a skill; picking a row or Escape closes the flow.
import { act } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { QueryResult } from "../transport/bindings.ts";
import { MATTER } from "../testing/stream-script.ts";
import type { AddonView } from "./addons.ts";
import { FakeSkillLinks } from "./add-skill-fake.ts";
import { mountRoom, type Harness } from "../room/thread/harness.ts";
import { SEEN_KEY } from "./seen.ts";

let h: Harness | null = null;
beforeEach(() => localStorage.removeItem(SEEN_KEY));
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const item = (id: string, source: AddonView["source"] = "added"): AddonView => ({
  kind: "skill",
  id,
  name: id,
  description: "A line.",
  source,
  scope: "all",
  on: true,
  status: "ready",
  can_remove: source === "added",
  locked: false,
  switched: [],
});

async function mount() {
  const links = new FakeSkillLinks();
  let reads = 0;
  h = await mountRoom({
    next: { parts: ["app"], query: () => ({ threads: [] }) },
    query: async (query) => {
      if (typeof query === "object" && "list_addons" in query) {
        reads += 1;
        const items = [
          item("legal-drafting", "built_in"),
          item("pdf"),
          ...links.added.map((name) => item(name)),
        ];
        return { addons: { items } } as QueryResult;
      }
      const answer = await links.query(query);
      if (answer === null) throw new Error(`not scripted: ${JSON.stringify(query)}`);
      return answer;
    },
    dispatch: async (command) =>
      (await links.dispatch({ command_id: "c", command })) ?? ("done" as const),
  });
  await h.settle();
  const rows = [...h.host.querySelectorAll(".room-matters .room-matter-row")];
  const sharma = rows.find((r) => r.textContent?.includes("Sharma v. State of Delhi"));
  if (sharma !== undefined) await h.click(sharma);
  await h.click(h.one("library-entry"));
  await act(async () => {
    await vi.dynamicImportSettled();
  });
  await h.settle();
  return { links, reads: () => reads };
}

const type = async (text: string) => {
  const input = h!.one("add-skill-field") as HTMLInputElement;
  await act(async () => {
    Reflect.set(HTMLInputElement.prototype, "value", text, input);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await h!.settle();
};

test("Add a skill opens in the panel; a link previews, Add sends add_skills and the new rows list", async () => {
  const { links, reads } = await mount();
  expect(h!.one("library-add")).toBeNull();
  await h!.click(h!.one("library-add-skill"));
  const panel = h!.one("library-add")!;
  expect(panel.textContent).toContain("Add a skill");
  expect(panel.textContent).toContain("Nothing is added until you've seen what it holds.");
  await type("https://github.com/lexdesk/skills");
  await h!.click(h!.one("add-skill-look"));
  // the matter open last, first, and chosen as the lens is
  const radios = [...panel.querySelectorAll('[role="radio"]')];
  expect(radios.map((r) => r.parentElement?.textContent)).toEqual([
    "Only Sharma v. State of Delhi",
    "Every matter",
  ]);
  expect(radios[0]!.getAttribute("aria-checked")).toBe("true");
  const before = reads();
  await h!.click(h!.one("add-skill-add"));
  expect(links.asked.at(-1)).toEqual({
    add_skills: {
      token: "t1",
      pick: ["bail-applications", "cheque-bounce", "court-fees"],
      scope: { matter: MATTER },
    },
  });
  expect(reads()).toBeGreaterThan(before);
  // the panel closes and the rows are the host's list
  expect(h!.one("library-add")).toBeNull();
  const added = [...h!.host.querySelectorAll('[data-testid="library-row"]')].map((r) =>
    r.getAttribute("data-id"),
  );
  expect(added).toEqual([
    "legal-drafting",
    "pdf",
    "bail-applications",
    "cheque-bounce",
    "court-fees",
  ]);
});

test("Escape closes Add a skill and gives focus back to its button", async () => {
  await mount();
  await h!.click(h!.one("library-add-skill"));
  const input = h!.one("add-skill-field")!;
  await act(async () => {
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await h!.settle();
  expect(h!.one("library-add")).toBeNull();
  expect(document.activeElement).toBe(h!.one("library-add-skill"));
});

test("picking a row closes Add a skill; Connectors has none", async () => {
  await mount();
  await h!.click(h!.one("library-add-skill"));
  expect(h!.one("library-add")).not.toBeNull();
  await h!.click(h!.host.querySelector('[data-id="pdf"]'));
  expect(h!.one("library-add")).toBeNull();
  const conn = [...h!.host.querySelectorAll('[role="tab"]')].find(
    (t) => t.textContent === "Connectors",
  );
  await h!.click(conn);
  expect(h!.one("library-add-skill")).toBeNull();
});
