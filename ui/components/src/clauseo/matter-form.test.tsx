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

test("Create the matter waits for a name, then gives the host the words trimmed", async () => {
  const made: string[][] = [];
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
  expect(made).toEqual([["Acme Ltd v. State", "Acme Ltd", "3 March 2027"]]);
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

test("the form's words keep the voice: no banned word, and Hindi sentences end in the danda", () => {
  for (const [en, hi] of Object.entries(matterFormWords)) {
    expect(en).not.toMatch(/\bI\b|\b(pencil|ink|step|git|token|session)\b/i);
    expect(hi).not.toMatch(/पेंसिल|स्याही|चरण|कदम|(^|\s)मैं(\s|$)/);
    if (en.endsWith(".")) expect(hi.endsWith("।")).toBe(true);
  }
});
