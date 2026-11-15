// The first screen's Open the folder (folder PR 2): while the folder picked is made a matter, "Opening the folder…"
// under the button, which waits (a second press does nothing) until the host answers; made, or a matter the host
// already has for the folder, it opens; refused, its reason's line under the button, and for a folder nested with a
// matter on the list, Open {name} (RefusedLine, RefusedOpen); with no name, the refused line and no Open.
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { createErrorLine } from "@clauseo/components/clauseo/matter-form-check.ts";
import { HI } from "@clauseo/components/lib/words-matters.hi.ts";
import { expect, test, vi } from "vitest";
import type { CommandOutcome, HostError } from "../../transport/protocol.ts";
import type { Matters } from "../matters.ts";
import { RefusedLine, RefusedOpen, type Refused } from "../new-matter.tsx";
import { A, MADE_MATTER, byId, mountFirstRun, press, sent, settle, text } from "./rig.tsx";

const OPENING = "Opening the folder…";
const PICKED = { folder: "folder-ref-1", display: "Documents/Matters/Rathi Steels" };
/** The refused line, naming the folder picked. */
const CANT = `Clauseo can't make a matter of \u2068~/${PICKED.display}\u2069. Choose another folder.`;
const title = () => document.querySelector('[data-testid="matter"]')?.textContent;

test("while the folder is made a matter: its line shows, the button waits, a second press does nothing, and the made matter opens", async () => {
  let answer: (outcome: CommandOutcome) => void = () => {};
  let picks = 0;
  const transport = await mountFirstRun({
    matters: [],
    create: () =>
      new Promise<CommandOutcome>((resolve) => {
        answer = resolve;
      }),
    listAfter: [{ matter: MADE_MATTER, name: "Rathi Steels", kind: "folder" }],
  });
  const original = transport.pickFolder.bind(transport);
  transport.pickFolder = async () => {
    picks += 1;
    return original();
  };
  expect(byId("folder-opening")).toBeNull();
  await press(byId("open-folder"));
  expect(byId("folder-opening")?.textContent).toBe(OPENING);
  const open = byId<HTMLButtonElement>("open-folder");
  expect(open?.disabled).toBe(true);
  expect(open?.getAttribute("aria-busy")).toBe("true");
  // pressed again, even past the disabled state: no second pick, no second create
  await act(async () => open?.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  await settle();
  expect(picks).toBe(1);
  expect(sent(transport, "create_matter")).toHaveLength(1);
  await act(async () => answer({ matter_created: { matter: MADE_MATTER } }));
  await settle();
  expect(text()).not.toContain(OPENING);
  expect(title()).toBe("Rathi Steels");
});

test("a matter the host already has for the folder (MatterCreated with it) opens, as a made one does", async () => {
  const transport = await mountFirstRun({
    matters: [],
    create: () => ({ matter_created: { matter: A.matter } }),
    listAfter: [A],
  });
  await press(byId("open-folder"));
  expect(sent(transport, "create_matter")).toHaveLength(1);
  expect(title()).toBe(A.name);
  expect(text()).not.toContain(OPENING);
});

const refusing = (refusal: Omit<HostError, "message" | "retryable">) => (): CommandOutcome => {
  throw { message: "SECRET-DETAIL", retryable: false, ...refusal };
};

/** Each place refusal's line, naming the folder picked. */
const LINES = {
  "matter.folder_is_drive": "{path} is the top of a drive. Choose a folder on it.",
  "matter.folder_is_home": "{path} is your home folder. Choose a folder inside it.",
  "matter.folder_is_system": "{path} is a system folder. Choose another folder.",
  "matter.folder_in_clauseo_data": "Clauseo keeps its own files in {path}. Choose another folder.",
  "matter.folder_too_deep":
    "{path} is too deeply nested for Windows. Choose a folder with a shorter path.",
} as const;

for (const lang of ["en", "hi"] as const) {
  for (const [code, en] of Object.entries(LINES) as [keyof typeof LINES, string][]) {
    test(`${code} says its reason under the button, and the button is free again (${lang})`, async () => {
      await mountFirstRun({ matters: [], language: lang, create: refusing({ code }) });
      await press(byId("open-folder"));
      expect(document.querySelector(".fl-refusal-head")?.textContent).toBe(
        (lang === "en" ? en : HI[en as keyof typeof HI]).replace(
          "{path}",
          `\u2068~/${PICKED.display}\u2069`,
        ),
      );
      expect(text()).not.toContain("SECRET-DETAIL");
      expect(byId<HTMLButtonElement>("open-folder")?.disabled).toBe(false);
      expect(byId("folder-opening")).toBeNull();
      expect(byId("open-matter")).toBeNull();
    });
  }
}

test("the matters root keeps its lines, with the folder's path", async () => {
  await mountFirstRun({ matters: [], create: refusing({ code: "matter.folder_in_matters_root" }) });
  await press(byId("open-folder"));
  expect(document.querySelector(".fl-refusal-head")?.textContent).toBe(
    createErrorLine("en", "matter.folder_in_matters_root", PICKED.display),
  );
});

for (const refusal of [
  { code: "matter.folder_inside_matter", matter: A.matter },
  { code: "matter.folder_holds_matter" },
  { code: "matter.folder_is_matter", matter: A.matter },
] as const) {
  test(`${refusal.code} with no matter on the list says the refused line, naming the folder, with no Open`, async () => {
    await mountFirstRun({ matters: [], create: refusing(refusal) });
    await press(byId("open-folder"));
    expect(document.querySelector(".fl-refusal-head")?.textContent).toBe(CANT);
    expect(text()).not.toMatch(/already a matter/);
    expect(byId("open-matter")).toBeNull();
  });
}

/** RefusedLine and RefusedOpen over a matters list holding A. */
async function renderRefused(refused: Refused) {
  const open = vi.fn();
  const matters = {
    view: () => ({ matters: [{ ...A }] }),
    open,
  } as unknown as Matters;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(
      <>
        <p>
          <RefusedLine matters={matters} refused={refused} />
        </p>
        <RefusedOpen matters={matters} refused={refused} />
      </>,
    ),
  );
  return { host, root, open };
}

test("a folder nested with a matter on the list, or one it already uses, names it on the first screen, and Open {name} opens it", async () => {
  for (const [code, words] of [
    [
      "matter.folder_inside_matter",
      "{path} is inside the folder of {name}. Choose another folder.",
    ],
    ["matter.folder_holds_matter", "{path} contains the folder of {name}. Choose another folder."],
    [
      "matter.folder_is_matter",
      "{name} already uses {path}. Open {name}, or choose another folder.",
    ],
  ] as const) {
    const { host, root, open } = await renderRefused({ code, matter: A.matter, path: "Z:\\Acme" });
    expect(host.querySelector("p")?.textContent).toBe(
      words.replace("{path}", "\u2068Z:\\Acme\u2069").replaceAll("{name}", `\u2068${A.name}\u2069`),
    );
    const button = host.querySelector<HTMLButtonElement>('[data-testid="open-matter"]');
    expect(button?.textContent).toBe(`Open \u2068${A.name}\u2069`);
    await act(async () => button?.click());
    expect(open).toHaveBeenCalledWith(A.matter);
    await act(async () => root.unmount());
    host.remove();
  }
  // a matter not on the list: the refused line, no Open
  const { host, root } = await renderRefused({
    code: "matter.folder_inside_matter",
    matter: "00000000-0000-4000-8000-000000000042",
    path: "Z:\\Acme",
  });
  expect(host.querySelector("p")?.textContent).toBe(
    "Clauseo can't make a matter of \u2068Z:\\Acme\u2069. Choose another folder.",
  );
  expect(host.querySelector('[data-testid="open-matter"]')).toBeNull();
  await act(async () => root.unmount());
  host.remove();
});
