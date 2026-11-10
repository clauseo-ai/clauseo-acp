// @vitest-environment happy-dom
// The new matter form and its dialog: Create matter waits for a name and a folder; the Folder row with no folder, a
// folder the picker gave (home as "~", the whole path in the text and its title, the last part kept), and the new folder
// in Clauseo's place; Choose folder… opening the picker at once; Enter before the form is ready saying what's missing;
// the refusal's one way on; and the dialog opening with focus on the name, Tab going name, client, Choose folder…,
// Create, and closing on Cancel.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { matterFormWords } from "../lib/words-matter-form.hi.ts";
import { homeShown, splitPath, under } from "./folder-path.ts";
import { NewMatterForm } from "./matter-form.tsx";
import { NewMatterDialog } from "./new-matter-dialog.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  for (const n of document.body.querySelectorAll("[data-base-ui-portal], [role=dialog]"))
    n.remove();
});

const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent?.trim() === label,
  );
const type = async (input: HTMLInputElement | undefined, value: string) => {
  await act(async () => {
    if (input !== undefined) Reflect.set(HTMLInputElement.prototype, "value", value, input);
    input?.dispatchEvent(new Event("input", { bubbles: true }));
  });
};
const byTestId = (id: string) =>
  document.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`) ?? undefined;
const inputs = () => [...document.querySelectorAll<HTMLInputElement>(".matter-form input")];
const path = () => document.querySelector<HTMLElement>("[data-testid=matter-folder-path]");
const create = () => button("Create matter");
const choose = () => button("Choose folder…");
const IDS = { name: "matter-name", existing: "matter-existing", made: "matter-made" };

/** A promise and the hands that settle it, so a test holds a picker or a create open. */
function held<T>() {
  let resolve: (v: T) => void = () => {};
  let reject: (e: unknown) => void = () => {};
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

test("paths: home as ~, a full path as it is, and the last part apart from its parent", () => {
  expect(homeShown("Documents/Clients/Acme")).toBe("~/Documents/Clients/Acme");
  expect(homeShown("Documents\\Clients\\Acme")).toBe("~\\Documents\\Clients\\Acme");
  expect(homeShown("/Volumes/Work/Acme")).toBe("/Volumes/Work/Acme");
  expect(homeShown("D:\\Cases")).toBe("D:\\Cases");
  expect(homeShown("\\\\server\\share")).toBe("\\\\server\\share");
  expect(under("~/Documents/Clauseo/", "Acme")).toBe("~/Documents/Clauseo/Acme");
  expect(splitPath("~/Documents/Clients/Acme")).toEqual({
    head: "~/Documents/Clients/",
    tail: "Acme",
  });
  expect(splitPath("D:\\Cases\\Acme")).toEqual({ head: "D:\\Cases\\", tail: "Acme" });
  expect(splitPath("/")).toEqual({ head: "/", tail: "" });
  expect(splitPath("Acme")).toEqual({ head: "", tail: "Acme" });
});

test("Create matter waits for a name and a folder, then gives the host the words trimmed", async () => {
  const made: unknown[][] = [];
  await mount(
    <NewMatterForm pickFolder={() => "Documents/Acme"} onCreate={(...a) => made.push(a)} />,
  );
  const [name, client] = inputs();
  expect(create()?.disabled).toBe(true);
  await type(name, "   ");
  expect(create()?.disabled).toBe(true);
  await type(name, " Acme share purchase ");
  // a name, no folder: still waiting
  expect(create()?.disabled).toBe(true);
  await act(async () => choose()?.click());
  expect(create()?.disabled).toBe(false);
  await type(client, " Acme Holdings ");
  await act(async () => create()?.click());
  expect(made).toEqual([["Acme share purchase", "Acme Holdings", false]]);
  // a folder, no name: waiting again
  await type(name, "");
  expect(create()?.disabled).toBe(true);
});

test("no folder: the row says so, Choose folder… is the main button, and no title stands over nothing", async () => {
  await mount(<NewMatterForm name="Acme" pickFolder={() => null} onCreate={() => {}} />);
  expect(path()?.textContent).toBe("No folder chosen");
  expect(document.querySelector(".matter-form-folder-field")?.getAttribute("title")).toBeNull();
  expect(choose()?.className).toContain("bg-primary");
  // a cancelled picker keeps nothing chosen
  await act(async () => choose()?.click());
  expect(path()?.textContent).toBe("No folder chosen");
  expect(create()?.disabled).toBe(true);
});

test("a folder chosen: home as ~, the whole path in the text and the title, the last part kept apart", async () => {
  const long =
    "Documents/Clients/Acme Holdings Private Limited/Corporate/2026 Share purchase and disclosure schedules/Working papers";
  await mount(<NewMatterForm name="Acme" folder={long} onCreate={() => {}} />);
  const full = `~/${long}`;
  // the whole path is in the text, for a screen reader, and under the pointer
  expect(path()?.textContent).toBe(full);
  expect(document.querySelector(".matter-form-folder-field")?.getAttribute("title")).toBe(full);
  // isolated, in English, the parent cut short by CSS before the folder's own name
  const bdi = path()?.querySelector("bdi");
  expect(bdi?.getAttribute("dir")).toBe("ltr");
  expect(bdi?.getAttribute("lang")).toBe("en");
  expect(bdi?.querySelector(".folder-path-head")?.textContent).toBe(
    "~/Documents/Clients/Acme Holdings Private Limited/Corporate/2026 Share purchase and disclosure schedules/",
  );
  expect(bdi?.querySelector(".folder-path-tail")?.textContent).toBe("Working papers");
  // Choose folder… steps down to a raised button, and is described by the path
  expect(choose()?.className).not.toContain("bg-primary");
  expect(choose()?.getAttribute("aria-describedby")).toBe(path()?.id);
});

test("Choose folder… opens the picker at once, waits for it, and a cancel keeps what was there", async () => {
  const pick = held<string | null>();
  let asked = 0;
  await mount(
    <NewMatterForm
      name="Acme"
      testIds={IDS}
      pickFolder={() => {
        asked += 1;
        return pick.promise;
      }}
      onCreate={() => {}}
    />,
  );
  const button = byTestId("matter-existing");
  await act(async () => button?.click());
  expect(asked).toBe(1);
  expect(button?.disabled).toBe(true);
  await act(async () => pick.resolve("Documents/Acme"));
  expect(path()?.textContent).toBe("~/Documents/Acme");
  expect(button?.disabled).toBe(false);
  await act(async () => button?.click());
  expect(asked).toBe(2);
});

test("Use a new folder: offered under the row, it shows the folder Clauseo makes, named as the matter", async () => {
  const made: unknown[][] = [];
  let told = 0;
  await mount(
    <NewMatterForm
      location="Documents/Clauseo"
      testIds={IDS}
      pickFolder={() => "Documents/Acme"}
      onMade={() => {
        told += 1;
      }}
      onCreate={(...a) => made.push(a)}
    />,
  );
  const offer = byTestId("matter-made");
  expect(offer?.textContent).toBe("Use a new folder in \u2068~/Documents/Clauseo\u2069");
  await act(async () => offer?.click());
  expect(told).toBe(1);
  expect(byTestId("matter-made")).toBeUndefined();
  expect(path()?.textContent).toBe("~/Documents/Clauseo/…");
  expect(document.querySelector(".matter-form-folder-note")?.textContent).toBe(
    "Clauseo makes this folder. Add papers to it any time.",
  );
  await type(inputs()[0], "Acme lease");
  expect(path()?.textContent).toBe("~/Documents/Clauseo/Acme lease");
  await act(async () => create()?.click());
  expect(made).toEqual([["Acme lease", "", true]]);
  // a folder picked afterwards replaces it, and the offer comes back
  await act(async () => choose()?.click());
  expect(path()?.textContent).toBe("~/Documents/Acme");
  expect(byTestId("matter-made")).toBeDefined();
});

test("no place for new matters, no offer", async () => {
  await mount(<NewMatterForm testIds={IDS} pickFolder={() => null} onCreate={() => {}} />);
  expect(byTestId("matter-made")).toBeUndefined();
});

test("Enter before the form is ready says what's missing under each field, and sends nothing", async () => {
  const made: unknown[][] = [];
  await mount(
    <NewMatterForm pickFolder={() => "Documents/Acme"} onCreate={(...a) => made.push(a)} />,
  );
  const [name] = inputs();
  const enter = () =>
    act(async () => {
      name?.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
  await enter();
  const lines = () =>
    [...document.querySelectorAll(".matter-form-error")].map((e) => e.textContent);
  expect(lines()).toEqual(["Name the matter.", "Choose a folder for the matter."]);
  expect(name?.getAttribute("aria-invalid")).toBe("true");
  expect(made).toEqual([]);
  await type(name, "Acme");
  expect(lines()).toEqual(["Choose a folder for the matter."]);
  // the folder's line describes Choose folder…
  const describedBy = choose()?.getAttribute("aria-describedby") ?? "";
  expect(describedBy.split(" ")).toContain(document.querySelectorAll(".matter-form-error")[0]?.id);
  await act(async () => choose()?.click());
  expect(lines()).toEqual([]);
});

test("without details, the form asks the name alone, and gives no client", async () => {
  const made: unknown[][] = [];
  await mount(
    <NewMatterForm
      details={false}
      name="Acme"
      client="Old"
      folder="Documents/Acme"
      onCreate={(...a) => made.push(a)}
    />,
  );
  expect(inputs()).toHaveLength(1);
  await act(async () => create()?.click());
  expect(made).toEqual([["Acme", "", false]]);
});

test("Create stands pressed while the create runs, and the caller's error shows as an alert", async () => {
  const run = held<void>();
  let calls = 0;
  await mount(
    <NewMatterForm
      name="Acme"
      folder="Documents/Acme"
      error="Clauseo couldn't create the matter. Try again, or choose another folder."
      onCreate={() => {
        calls += 1;
        return run.promise;
      }}
    />,
  );
  expect(document.querySelector("[role=alert]")?.textContent).toBe(
    "Clauseo couldn't create the matter. Try again, or choose another folder.",
  );
  await act(async () => create()?.click());
  expect(create()?.disabled).toBe(true);
  expect(create()?.getAttribute("aria-busy")).toBe("true");
  await act(async () => create()?.click());
  expect(calls).toBe(1);
  await act(async () => run.reject(new Error("refused")));
  expect(create()?.disabled).toBe(false);
});

test("a refusal's one way on: Open it for a matter on the list, else Choose another folder for a folder refused", async () => {
  const line = "This folder is already a matter: Acme.";
  let opened = 0;
  let picks = 0;
  const pickFolder = () => {
    picks += 1;
    return "Documents/Other";
  };
  await mount(
    <NewMatterForm
      name="Acme"
      folder="Documents/Acme"
      error={line}
      folderError
      pickFolder={pickFolder}
      onCreate={() => {}}
      openMatter={() => {
        opened += 1;
      }}
      testIds={{ open: "open-matter" }}
    />,
  );
  expect(document.querySelector("[role=alert]")?.textContent).toBe(line);
  expect(button("Choose another folder")).toBeUndefined();
  await act(async () => byTestId("open-matter")?.click());
  expect(opened).toBe(1);
  await act(async () => root?.unmount());
  host?.remove();
  await mount(
    <NewMatterForm
      name="Acme"
      folder="Documents/Acme"
      error="Clauseo can't read ~/Documents/Acme."
      folderError
      pickFolder={pickFolder}
      onCreate={() => {}}
    />,
  );
  expect(button("Open it")).toBeUndefined();
  await act(async () => button("Choose another folder")?.click());
  expect(picks).toBe(1);
  expect(path()?.textContent).toBe("~/Documents/Other");
  // a line that isn't about the folder: no button under it
  await act(async () => root?.unmount());
  host?.remove();
  await mount(
    <NewMatterForm
      name="Acme"
      folder="Documents/Acme"
      error="x"
      pickFolder={pickFolder}
      onCreate={() => {}}
    />,
  );
  expect(button("Choose another folder")).toBeUndefined();
});

test("the dialog opens with focus on the name, Tab runs name, client, Choose folder…, Create, and Cancel closes it", async () => {
  const changes: boolean[] = [];
  await mount(
    <NewMatterDialog
      open
      folder="Documents/Acme"
      onOpenChange={(o) => changes.push(o)}
      pickFolder={() => null}
      onCreate={() => {}}
    />,
  );
  expect(document.querySelector("[role=dialog] .matter-form")).not.toBeNull();
  // Base UI moves the starting focus on the next frames, so wait for them, not for a clock
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise<void>((r) => requestAnimationFrame(() => r())));
  expect(document.activeElement?.tagName).toBe("INPUT");
  expect(document.activeElement?.closest("[role=dialog]")).not.toBeNull();
  // the document's order is the Tab order: no tabindex reorders it
  const order = [
    ...document.querySelectorAll<HTMLElement>("[role=dialog] input, [role=dialog] button"),
  ].map((e) => e.textContent?.trim() || e.tagName);
  expect(order).toEqual(["INPUT", "INPUT", "Choose folder…", "Create matter", "Cancel"]);
  expect(
    document.querySelector("[role=dialog] [tabindex]:not([tabindex='-1']):not([tabindex='0'])"),
  ).toBeNull();
  await act(async () => button("Cancel")?.click());
  expect(changes).toEqual([false]);
});

test("the dialog closes only when an async create succeeds", async () => {
  const changes: boolean[] = [];
  let answer: unknown = false;
  await mount(
    <NewMatterDialog
      open
      name="Acme"
      details={false}
      folder="Documents/Acme"
      onOpenChange={(o) => changes.push(o)}
      onCreate={async () => answer}
    />,
  );
  await act(async () => create()?.click());
  expect(changes).toEqual([]);
  answer = undefined;
  await act(async () => create()?.click());
  expect(changes).toEqual([false]);
});

test("the form's words keep the voice: no banned word, and Hindi sentences end in the danda", () => {
  for (const [en, hi] of Object.entries(matterFormWords)) {
    if (hi === null) continue; // hi: later, no Hindi yet
    expect(en).not.toMatch(/\bI\b|\b(pencil|ink|step|git|token|session|agent)\b/i);
    expect(en).not.toMatch(/next date|cause list|hearing|court/i);
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});

test("while the folder picker opens, a polite status line says so under the folder row, and only while it is told", async () => {
  await mount(<NewMatterForm folder="Documents/Acme" name="Acme" onCreate={() => {}} />);
  expect(byTestId("picker-opening")).toBeUndefined();
  await act(async () =>
    root?.render(<NewMatterForm folder="Documents/Acme" name="Acme" opening onCreate={() => {}} />),
  );
  const line = document.querySelector<HTMLElement>('[data-testid="picker-opening"]');
  expect(line?.getAttribute("role")).toBe("status");
  expect(line?.textContent).toBe("Opening the folder picker…");
  expect(line?.closest("button")).toBeNull();
  expect(line?.closest(".matter-form-folder")).toBeNull();
});
