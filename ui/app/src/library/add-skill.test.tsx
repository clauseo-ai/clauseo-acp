// @vitest-environment happy-dom
// Add a skill (RC A1) over FakeTransport and its scripted host (add-skill-fake.ts): each route, a
// link, a skills.sh name (typed, or picked from the matches with the keys) and a zip, previews with
// preview_skill and adds with add_skills; untick a skill and only the ticked go; each refusal's
// words read in the panel, Try again and Look again preview again; the busy words while the host
// works.
import { settled } from "@clauseo/components/lib/late.ts";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import type { AddonScope, HostCommand, HostQuery } from "../transport/bindings.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { AddSkillPanel } from "./add-skill.tsx";
import { readField, SEARCH_WAIT_MS } from "./add-skill-model.ts";
import { FakeSkillLinks, LEXDESK, ZIPPED } from "./add-skill-fake.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let el: HTMLElement | null = null;
afterEach(async () => {
  await act(async () => root?.unmount());
  el?.remove();
  root = null;
  el = null;
});

const SHARMA = "11111111-1111-4111-8111-111111111111";

async function mount(matter: boolean = false, scope0: AddonScope = "all") {
  const links = new FakeSkillLinks();
  const picks: boolean[] = [];
  const transport = new FakeTransport({
    query: async (query) => {
      const answer = await links.query(query);
      if (answer === null) throw new Error("not scripted");
      return answer;
    },
    dispatch: async (envelope) => (await links.dispatch(envelope)) ?? "done",
    pickSkill: (files) => {
      picks.push(files === true);
      return files === true ? "/Users/r/Downloads/lease-review.zip" : "/Users/r/Skills/notices";
    },
  });
  const added = vi.fn();
  el = document.createElement("div");
  document.body.append(el);
  root = createRoot(el);
  await act(async () =>
    root!.render(
      <AddSkillPanel
        transport={transport}
        matter={matter ? { id: SHARMA, name: "Sharma v. State of Delhi" } : null}
        scope0={scope0}
        onAdded={added}
      />,
    ),
  );
  const settle = async () => {
    for (let i = 0; i < 4; i += 1) await act(async () => settled());
  };
  const q = <E extends HTMLElement = HTMLElement>(id: string) =>
    el!.querySelector<E>(`[data-testid="${id}"]`);
  const click = async (target: HTMLElement | null) => {
    if (target === null) throw new Error("nothing to click");
    await act(async () => target.click());
    await settle();
  };
  const type = async (text: string) => {
    const input = q<HTMLInputElement>("add-skill-field")!;
    await act(async () => {
      Reflect.set(HTMLInputElement.prototype, "value", text, input);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await settle();
  };
  const key = async (k: string) => {
    const input = q("add-skill-field")!;
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    });
    await settle();
  };
  const skills = () =>
    [...el!.querySelectorAll('[data-testid="add-skill-skill"]')].map(
      (s) => s.querySelector(".add-skill-skill-name")?.textContent ?? "",
    );
  const ticks = () =>
    [...el!.querySelectorAll('[data-testid="add-skill-skill"] [role="checkbox"]')] as HTMLElement[];
  const previews = () =>
    links.asked.flatMap((a) =>
      typeof a === "object" && "preview_skill" in a && a.preview_skill !== undefined
        ? [a.preview_skill.from]
        : [],
    );
  const adds = () =>
    links.asked.flatMap((a) =>
      typeof a === "object" && "add_skills" in a && a.add_skills !== undefined
        ? [a.add_skills]
        : [],
    );
  return {
    links,
    transport,
    picks,
    added,
    q,
    click,
    type,
    key,
    settle,
    skills,
    ticks,
    previews,
    adds,
  };
}

test("the field reads a link, a bare GitHub link, a skills.sh name, and words", () => {
  expect(readField(" https://github.com/lexdesk/skills ")).toEqual({
    kind: "from",
    from: { url: "https://github.com/lexdesk/skills" },
  });
  expect(readField("github.com/lexdesk/skills/tree/main/bail")).toEqual({
    kind: "from",
    from: { url: "https://github.com/lexdesk/skills/tree/main/bail" },
  });
  expect(readField("lexdesk/skills")).toEqual({
    kind: "from",
    from: { skills_sh: "lexdesk/skills" },
  });
  expect(readField("lexdesk/skills@v2/bail-applications")).toEqual({
    kind: "from",
    from: { skills_sh: "lexdesk/skills@v2/bail-applications" },
  });
  expect(readField("contract review")).toEqual({ kind: "words", q: "contract review" });
  expect(readField("  ")).toEqual({ kind: "empty" });
});

test("a GitHub link previews with preview_skill {url}, every skill ticked, and Add sends add_skills", async () => {
  const p = await mount();
  expect(p.q("add-skill-look")?.hasAttribute("disabled")).toBe(true);
  await p.type("https://github.com/lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  expect(p.previews()).toEqual([{ url: "https://github.com/lexdesk/skills" }]);
  expect(p.q("add-skill-preview")?.textContent).toContain(
    "From skills.sh, lexdesk/skills It holds 3 skills. Tick the ones to add.",
  );
  expect(p.skills()).toEqual(["bail-applications", "cheque-bounce", "court-fees"]);
  expect(p.ticks().map((t) => t.getAttribute("aria-checked"))).toEqual(["true", "true", "true"]);
  expect(p.q("add-skill-preview")?.textContent).toContain(
    "You have this one. Adding it again replaces yours.",
  );
  expect(p.q("add-skill-trust")?.textContent).toContain("Add only skills you trust.");
  await p.click(p.q("add-skill-add"));
  expect(p.adds()).toEqual([
    { token: "t1", pick: ["bail-applications", "cheque-bounce", "court-fees"], scope: "all" },
  ]);
  expect(p.added).toHaveBeenCalledTimes(1);
  // back to the empty field
  expect(p.q("add-skill-preview")).toBeNull();
  expect(p.q<HTMLInputElement>("add-skill-field")?.value).toBe("");
});

test("a skills.sh name previews with preview_skill {skills_sh} on Enter", async () => {
  const p = await mount();
  await p.type("lexdesk/skills");
  await p.key("Enter");
  expect(p.previews()).toEqual([{ skills_sh: "lexdesk/skills" }]);
  expect(p.skills()).toHaveLength(3);
});

test("words show skills.sh's matches after a pause; the arrows and Enter preview one", async () => {
  const p = await mount();
  await p.type("skills");
  expect(p.links.asked).toEqual([]);
  await act(async () => new Promise((r) => setTimeout(r, SEARCH_WAIT_MS + 50)));
  await p.settle();
  expect(p.links.asked).toEqual([{ search_skills: { q: "skills" } }] satisfies HostQuery[]);
  const input = p.q("add-skill-field")!;
  expect(input.getAttribute("aria-expanded")).toBe("true");
  const options = [...document.querySelectorAll('[data-testid="add-skill-match"]')];
  expect(options.map((o) => o.textContent)).toEqual([
    "contract-reviewlexdesk/skills",
    "docxanthropics/skills",
  ]);
  await p.key("ArrowDown");
  await p.key("ArrowDown");
  expect(input.getAttribute("aria-activedescendant")).toBe(options[1]!.id);
  expect(options[1]!.getAttribute("aria-selected")).toBe("true");
  await p.key("Enter");
  expect(p.previews()).toEqual([{ skills_sh: "anthropics/skills/docx" }]);
  expect(input.getAttribute("aria-expanded")).toBe("false");
});

test("Choose a zip opens the file dialog and previews with preview_skill {path}; one skill has no tick", async () => {
  const p = await mount();
  p.links.previews.set(JSON.stringify({ path: "/Users/r/Downloads/lease-review.zip" }), ZIPPED);
  await p.click(p.q("add-skill-zip"));
  expect(p.picks).toEqual([true]);
  expect(p.previews()).toEqual([{ path: "/Users/r/Downloads/lease-review.zip" }]);
  expect(p.q("add-skill-preview")?.textContent).toContain(
    "From a zip file on this computer It holds one skill.",
  );
  expect(p.ticks()).toHaveLength(0);
  await p.click(p.q("add-skill-add"));
  expect(p.adds()).toEqual([{ token: "t1", pick: ["lease-review"], scope: "all" }]);
});

test("Choose a folder previews the folder with preview_skill {path}", async () => {
  const p = await mount();
  await p.click(p.q("add-skill-folder"));
  expect(p.picks).toEqual([false]);
  expect(p.previews()).toEqual([{ path: "/Users/r/Skills/notices" }]);
});

test("unticked skills stay out: add_skills names only the ticked, with the matter chosen", async () => {
  const p = await mount(true);
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  await p.click(p.ticks()[1]!);
  // the matter alone
  const only = [...document.querySelectorAll('[role="radio"]')][0] as HTMLElement;
  expect(only.parentElement?.textContent).toBe("Only Sharma v. State of Delhi");
  await p.click(only);
  await p.click(p.q("add-skill-add"));
  expect(p.adds()).toEqual([
    { token: "t1", pick: ["bail-applications", "court-fees"], scope: { matter: SHARMA } },
  ] satisfies Extract<HostCommand, { add_skills: unknown }>["add_skills"][]);
});

test("with no skill ticked, Add waits", async () => {
  const q = await mount();
  await q.type("lexdesk/skills");
  await q.click(q.q("add-skill-look"));
  for (const t of q.ticks()) await q.click(t);
  expect(q.q("add-skill-add")?.hasAttribute("disabled")).toBe(true);
});

test.each([
  [
    "skill.download_failed",
    "Clauseo couldn't download that skill. Check the link or your connection, then try again.",
    "Try again",
  ],
  [
    "skill.not_found",
    "There's nothing at that link or name. Check it against the page you copied it from.",
    null,
  ],
  ["skill.too_big", "That skill is too big to add. Ask whoever shared it for a smaller one.", null],
  ["skill.invalid", "Clauseo couldn't add that skill, so nothing was added.", null],
])("a preview refused with %s says so in the panel", async (code, words, mend) => {
  const p = await mount();
  p.links.refuse.preview = code;
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  const trouble = p.q("add-skill-trouble")!;
  expect(trouble.getAttribute("role")).toBe("alert");
  expect(trouble.querySelector("p")?.textContent).toBe(words);
  expect(p.q("add-skill-again")?.textContent ?? null).toBe(mend);
  if (mend === null) return;
  await p.click(p.q("add-skill-again"));
  expect(p.previews()).toEqual([{ skills_sh: "lexdesk/skills" }, { skills_sh: "lexdesk/skills" }]);
  expect(p.q("add-skill-trouble")).toBeNull();
  expect(p.skills()).toHaveLength(3);
});

test("an expired preview: its words, Add held, Look again previews again and keeps the ticks", async () => {
  const p = await mount();
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  await p.click(p.ticks()[0]!);
  p.links.refuse.add = "skill.preview_expired";
  await p.click(p.q("add-skill-add"));
  expect(p.q("add-skill-trouble")?.querySelector("p")?.textContent).toBe(
    "This preview is out of date. Look again to see what it holds now.",
  );
  expect(p.q("add-skill-add")?.hasAttribute("disabled")).toBe(true);
  expect(p.q("add-skill-again")?.textContent).toBe("Look again");
  await p.click(p.q("add-skill-again"));
  expect(p.previews()).toHaveLength(2);
  expect(p.ticks().map((t) => t.getAttribute("aria-checked"))).toEqual(["false", "true", "true"]);
  await p.click(p.q("add-skill-add"));
  expect(p.adds().map((a) => [a.token, a.pick])).toEqual([
    ["t1", ["cheque-bounce", "court-fees"]],
    ["t2", ["cheque-bounce", "court-fees"]],
  ]);
  expect(p.added).toHaveBeenCalledTimes(1);
});

test("Previewing… and Adding… while the host works", async () => {
  const p = await mount();
  let open = () => {};
  p.links.gate.preview = new Promise<void>((r) => (open = r));
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  expect(p.q("add-skill-look")?.textContent).toBe("Previewing…");
  expect(p.q("add-skill-zip")?.hasAttribute("disabled")).toBe(true);
  await act(async () => open());
  await p.settle();
  expect(p.q("add-skill-look")?.textContent).toBe("Look at it");
  p.links.gate.add = new Promise<void>((r) => (open = r));
  await p.click(p.q("add-skill-add"));
  expect(p.q("add-skill-add")?.textContent).toBe("Adding…");
  expect(p.q("add-skill")?.getAttribute("aria-busy")).toBe("true");
  await act(async () => open());
  await p.settle();
  expect(p.added).toHaveBeenCalledTimes(1);
});

test("Add failing on the way says it may have gone through; a host's refusal says nothing was added", async () => {
  const p = await mount();
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  p.links.refuse.add = "internal";
  await p.click(p.q("add-skill-add"));
  expect(p.q("add-skill-trouble")?.textContent).toBe(
    "Clauseo couldn't tell whether that skill was added. Look for it in the list before you add it again.",
  );
  p.links.refuse.add = "skill.invalid";
  await p.click(p.q("add-skill-add"));
  expect(p.q("add-skill-trouble")?.textContent).toBe(
    "Clauseo couldn't add that skill, so nothing was added.",
  );
});

test("pressing a skill's name ticks and unticks it", async () => {
  const p = await mount();
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  const name = document.querySelectorAll<HTMLElement>(".add-skill-skill-name")[2]!;
  await p.click(name);
  expect(p.ticks().map((t) => t.getAttribute("aria-checked"))).toEqual(["true", "true", "false"]);
  await p.click(name);
  expect(p.ticks()[2]!.getAttribute("aria-checked")).toBe("true");
});

test("Escape closes the matches first, then clears the panel", async () => {
  const p = await mount();
  await p.type("skills");
  await act(async () => new Promise((r) => setTimeout(r, SEARCH_WAIT_MS + 50)));
  await p.settle();
  const input = p.q("add-skill-field")!;
  expect(input.getAttribute("aria-expanded")).toBe("true");
  await p.key("Escape");
  expect(input.getAttribute("aria-expanded")).toBe("false");
  expect(p.q<HTMLInputElement>("add-skill-field")?.value).toBe("skills");
  await p.type("lexdesk/skills");
  await p.click(p.q("add-skill-look"));
  expect(p.q("add-skill-preview")).not.toBeNull();
  await p.key("Escape");
  expect(p.q("add-skill-preview")).toBeNull();
  expect(p.q<HTMLInputElement>("add-skill-field")?.value).toBe("");
});

test("two skills of one name both show, each with its own tick", async () => {
  const p = await mount();
  const twice = LEXDESK.skills[0]!;
  p.links.previews.set(JSON.stringify({ skills_sh: "lexdesk/twins" }), {
    ...LEXDESK,
    skills: [twice, { ...twice, description: "Another." }],
  });
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  await p.type("lexdesk/twins");
  await p.click(p.q("add-skill-look"));
  expect(p.skills()).toEqual(["bail-applications", "bail-applications"]);
  expect(p.ticks()).toHaveLength(2);
  expect(errors.mock.calls.flat().join(" ")).not.toContain("same key");
  errors.mockRestore();
});
