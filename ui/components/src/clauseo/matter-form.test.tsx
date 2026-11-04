// @vitest-environment happy-dom
// The new matter form and its dialog: Create the matter waits for a name, the words go to the host trimmed, the
// folder follows the name, and the dialog opens with focus on the name and closes on Cancel.
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { matterFormWords } from "../lib/words-matter-form.ts";
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
const folderFor = (name: string) => `Documents/Clauseo/${name}`;
const byTestId = (id: string) =>
  document.querySelector<HTMLButtonElement>(`[data-testid="${id}"]`) ?? undefined;

test("Create the matter waits for a name, then gives the host the words trimmed", async () => {
  const made: (string | null)[][] = [];
  await mount(<NewMatterForm folderFor={folderFor} onCreate={(...a) => made.push(a)} />);
  expect(button("Create the matter")?.disabled).toBe(true);
  const [name, client, next] = [
    ...document.querySelectorAll<HTMLInputElement>(".matter-form input"),
  ];
  await type(name, "   ");
  expect(button("Create the matter")?.disabled).toBe(true);
  await type(name, " Acme Ltd v. State ");
  await type(client, "Acme Ltd");
  await type(next, "3 March 2027");
  expect(document.querySelector(".matter-form-path")?.textContent).toBe(
    "Documents/Clauseo/Acme Ltd v. State",
  );
  await act(async () => button("Create the matter")?.click());
  expect(made).toEqual([["Acme Ltd v. State", "Acme Ltd", "3 March 2027", "2027-03-03"]]);
});

test("Change takes the folder the picker gives, and keeps it when the picker gives none", async () => {
  let pick: string | null = "Documents/Cases/Acme";
  await mount(
    <NewMatterForm
      folderFor={folderFor}
      name="Acme Ltd v. State"
      pickFolder={() => pick}
      onCreate={() => {}}
    />,
  );
  await act(async () => button("Change")?.click());
  expect(document.querySelector(".matter-form-path")?.textContent).toBe("Documents/Cases/Acme");
  pick = null;
  await act(async () => button("Change")?.click());
  expect(document.querySelector(".matter-form-path")?.textContent).toBe("Documents/Cases/Acme");
});

test("the dialog opens with focus on the name, and Cancel closes it", async () => {
  const changes: boolean[] = [];
  await mount(<NewMatterDialog open onOpenChange={(o) => changes.push(o)} onCreate={() => {}} />);
  expect(document.querySelector("[role=dialog] .matter-form")).not.toBeNull();
  // Base UI moves the starting focus on the next frames, so wait for them, not for a clock
  for (let i = 0; i < 3; i += 1)
    await act(async () => new Promise<void>((r) => requestAnimationFrame(() => r())));
  expect(document.activeElement?.tagName).toBe("INPUT");
  expect(document.activeElement?.closest("[role=dialog]")).not.toBeNull();
  await act(async () => button("Cancel")?.click());
  expect(changes).toEqual([false]);
});

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

test("without details, the form asks only the name, and gives no client or date", async () => {
  const made: unknown[][] = [];
  await mount(
    <NewMatterForm
      details={false}
      name="Acme"
      client="Old"
      next="nonsense"
      onCreate={(...a) => made.push(a)}
    />,
  );
  expect(document.querySelectorAll(".matter-form input")).toHaveLength(1);
  await act(async () => button("Create the matter")?.click());
  expect(made).toEqual([["Acme", "", "", null]]);
});

test("a next date that doesn't read says how to write it once the field is left, and holds Create back", async () => {
  await mount(<NewMatterForm name="Acme" onCreate={() => {}} />);
  const next = document.querySelectorAll<HTMLInputElement>(".matter-form input")[2];
  // half typed: Create waits, but nothing says the field is wrong yet
  await type(next, "3");
  expect(document.querySelector(".matter-form-error")).toBeNull();
  expect(next?.getAttribute("aria-invalid")).toBeNull();
  expect(button("Create the matter")?.disabled).toBe(true);
  await type(next, "next Tuesday");
  expect(document.querySelector(".matter-form-error")).toBeNull();
  await act(async () => next?.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
  expect(document.querySelector(".matter-form-error")?.textContent).toBe(
    "Write the date as 30 October 2026.",
  );
  expect(next?.getAttribute("aria-invalid")).toBe("true");
  expect(button("Create the matter")?.disabled).toBe(true);
  await type(next, "30 October 2026");
  expect(document.querySelector(".matter-form-error")).toBeNull();
  expect(button("Create the matter")?.disabled).toBe(false);
});

test("folder null shows Change alone; Change waits for the picker, and a cancel keeps what was there", async () => {
  const pick = held<string | null>();
  const asked: string[] = [];
  await mount(
    <NewMatterForm
      folder={null}
      name="Acme"
      testIds={{ name: "matter-name", existing: "matter-existing", create: "create-matter" }}
      pickFolder={(current) => {
        asked.push(current);
        return pick.promise;
      }}
      onCreate={() => {}}
    />,
  );
  expect(document.querySelector("[data-testid=matter-name]")?.tagName).toBe("INPUT");
  expect(document.querySelector("[data-testid=create-matter]")).not.toBeNull();
  expect(document.querySelector(".matter-form-path")).toBeNull();
  // no folder icon beside nothing
  expect(document.querySelector(".matter-form-folder-icon")).toBeNull();
  // no folder words on screen; Change is described, for a screen reader, by what it changes
  const words = document.querySelector(".matter-form-folder-words");
  expect(words?.querySelector(".sr-only")?.textContent).toBe("The matter's folder");
  expect(words?.textContent).toBe("The matter's folder");
  const change = document.querySelector<HTMLButtonElement>("[data-testid=matter-existing]");
  expect(change?.getAttribute("aria-describedby")).toBe(words?.id);
  await act(async () => change?.click());
  expect(asked).toEqual([""]);
  expect(change?.disabled).toBe(true);
  await act(async () => pick.resolve("Documents/Acme"));
  expect(document.querySelector(".matter-form-path")?.textContent).toBe("Documents/Acme");
  // the path is isolated, and Change is described by the folder line
  expect(document.querySelector(".matter-form-path")?.tagName).toBe("BDI");
  expect(document.getElementById(change?.getAttribute("aria-describedby") ?? "")?.textContent).toBe(
    "Folder: Documents/Acme",
  );
  expect(change?.disabled).toBe(false);
  await act(async () => change?.click());
  expect(asked).toEqual(["", "Documents/Acme"]);
});

test("a picker that gives nothing keeps the folder line hidden", async () => {
  await mount(
    <NewMatterForm folder={null} name="Acme" pickFolder={async () => null} onCreate={() => {}} />,
  );
  await act(async () => button("Change")?.click());
  expect(document.querySelector(".matter-form-path")).toBeNull();
});

test("Create stands pressed while the create runs, and the caller's error shows as an alert", async () => {
  const run = held<void>();
  let calls = 0;
  await mount(
    <NewMatterForm
      name="Acme"
      folder={null}
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
  await act(async () => button("Create the matter")?.click());
  const create = button("Create the matter");
  expect(create?.disabled).toBe(true);
  expect(create?.getAttribute("aria-busy")).toBe("true");
  await act(async () => create?.click());
  expect(calls).toBe(1);
  await act(async () => run.reject(new Error("refused")));
  expect(button("Create the matter")?.disabled).toBe(false);
});

test("Open it sits under the line only when the caller gives it, and presses through to the caller", async () => {
  const line = "This folder is already a matter: Acme.";
  let opened = 0;
  const onCreate = () => {};
  await mount(
    <NewMatterForm
      name="Acme"
      folder={null}
      error={line}
      onCreate={onCreate}
      openMatter={() => {
        opened += 1;
      }}
      testIds={{ open: "open-matter" }}
    />,
  );
  expect(document.querySelector("[role=alert]")?.textContent).toBe(line);
  await act(async () => byTestId("open-matter")?.click());
  expect(opened).toBe(1);
  // a line with no matter to open: no button
  await act(async () => root?.unmount());
  host?.remove();
  await mount(<NewMatterForm name="Acme" folder={null} error={line} onCreate={onCreate} />);
  expect(button("Open it")).toBeUndefined();
  // no line, no button, even with a matter to open
  await act(async () => root?.unmount());
  host?.remove();
  await mount(
    <NewMatterForm
      name="Acme"
      folder={null}
      onCreate={onCreate}
      openMatter={() => {}}
      testIds={{ open: "open-matter" }}
    />,
  );
  expect(byTestId("open-matter")).toBeUndefined();
});

test("the dialog closes only when an async create succeeds", async () => {
  const changes: boolean[] = [];
  let answer: unknown = false;
  await mount(
    <NewMatterDialog
      open
      name="Acme"
      details={false}
      folder={null}
      onOpenChange={(o) => changes.push(o)}
      onCreate={async () => answer}
    />,
  );
  await act(async () => button("Create the matter")?.click());
  expect(changes).toEqual([]);
  answer = undefined;
  await act(async () => button("Create the matter")?.click());
  expect(changes).toEqual([false]);
});

test("the form's words keep the voice: no banned word, and Hindi sentences end in the danda", () => {
  for (const [en, hi] of Object.entries(matterFormWords)) {
    if (hi === null) continue; // hi: later, no Hindi yet
    expect(en).not.toMatch(/\bI\b|\b(pencil|ink|step|git|token|session)\b/i);
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});
