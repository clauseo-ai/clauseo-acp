// @vitest-environment happy-dom
// The edge of the matter, rendered with props: the thread asks before Clauseo goes beyond the matter, the computer's
// own dialog confirms on the Mac and on Windows, protection switched off keeps its safe answer on both keys, Not
// protected is explained once, what the matter allows lists each place with Remove, and after refusals Clauseo stops.
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";
import { edgeDialogWords, type EdgeAsk } from "../lib/words-versions.ts";
import { AllowedList, WorkspaceLine } from "./allowed.tsx";
import { EdgePrompt, RefusalPause } from "./edge.tsx";
import { OsDialogPreview } from "./os-dialog.tsx";
import { NotProtectedNote } from "./protection.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot> | null = null;

async function show(node: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<StrictMode>{node}</StrictMode>));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

const one = <T extends Element>(selector: string, box: ParentNode = document) => {
  const n = box.querySelector<T>(selector);
  if (n === null) throw new Error(`no ${selector}`);
  return n;
};
/** A button by its words, without its key cap. */
const button = (text: string, box: ParentNode = document) => {
  const said = (x: HTMLElement) =>
    [...x.childNodes]
      .filter((n) => !(n instanceof HTMLElement && n.tagName === "KBD"))
      .map((n) => n.textContent)
      .join("")
      .trim();
  const b = [...box.querySelectorAll<HTMLButtonElement>("button")].find((x) => said(x) === text);
  if (b === undefined) throw new Error(`no button "${text}"`);
  return b;
};
const press = async (b: HTMLElement) => act(async () => b.click());
const key = async (on: Element, k: string) =>
  act(async () => {
    on.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
  });
const words = () => document.body.textContent ?? "";

test("a folder beyond the matter: the thread asks, Allow hands it to the computer, and the answer folds to one line", async () => {
  const onAllow = vi.fn();
  await show(
    <EdgePrompt
      kind="folder"
      place="Documents/Precedents"
      state={{ kind: "pending" }}
      platform="mac"
      keys
      onAllow={onAllow}
      onRefuse={() => {}}
    />,
  );
  expect(one(".edge").textContent).toContain(
    "Clauseo wants to read a folder outside this matter: Documents/Precedents.",
  );
  expect(one(".edge-allow").getAttribute("aria-keyshortcuts")).toBe("Enter");
  await press(button("Allow"));
  expect(onAllow).toHaveBeenCalledTimes(1);
  // Enter from where nothing is focused answers too
  await key(document.body, "Enter");
  expect(onAllow).toHaveBeenCalledTimes(2);
});

test("while the computer's dialog is up the prompt says so, and its keys rest", async () => {
  const onAllow = vi.fn();
  await show(
    <EdgePrompt
      kind="folder"
      place="Documents/Precedents"
      state={{ kind: "confirming" }}
      platform="mac"
      keys
      onAllow={onAllow}
      onRefuse={() => {}}
    />,
  );
  expect(one(".edge-confirming").textContent).toBe("macOS is asking you to confirm.");
  await key(document.body, "Enter");
  expect(onAllow).not.toHaveBeenCalled();
});

test("answered, the prompt is one past-tense line", async () => {
  await show(
    <>
      <EdgePrompt
        kind="folder"
        place="Documents/Precedents"
        state={{ kind: "allowed", remembered: true, at: "5:12 pm" }}
        platform="mac"
        onAllow={() => {}}
        onRefuse={() => {}}
      />
      <EdgePrompt
        kind="site"
        place="example.org"
        state={{ kind: "refused", remembered: false }}
        platform="windows"
        onAllow={() => {}}
        onRefuse={() => {}}
      />
    </>,
  );
  const done = [...document.querySelectorAll(".perm-done")].map((n) => n.textContent);
  expect(done).toEqual([
    "Allowed for this matter. Clauseo can read Documents/Precedents.",
    "Not allowed. Clauseo didn't open example.org.",
  ]);
  expect(document.querySelector(".edge .perm-actions")).toBeNull();
});

test("switching protection off: Enter and Escape both keep protection, and Switch off has no key", async () => {
  const onAllow = vi.fn();
  const onRefuse = vi.fn();
  await show(
    <EdgePrompt
      kind="off"
      place=""
      state={{ kind: "pending" }}
      platform="mac"
      keys
      onAllow={onAllow}
      onRefuse={onRefuse}
    />,
  );
  expect(one(".edge-allow").querySelector(".perm-key")).toBeNull();
  expect(one(".edge-allow").hasAttribute("aria-keyshortcuts")).toBe(false);
  expect(one(".edge-refuse").getAttribute("aria-keyshortcuts")).toBe("Enter Escape");
  await key(document.body, "Escape");
  await key(document.body, "Enter");
  expect(onRefuse).toHaveBeenCalledTimes(2);
  expect(onAllow).not.toHaveBeenCalled();
});

const dialog = (
  ask: EdgeAsk,
  platform: "mac" | "windows",
  on: { confirm?: () => void; remember?: (c: boolean) => void } = {},
) => {
  const d = edgeDialogWords("en", ask);
  return (
    <OsDialogPreview
      platform={platform}
      title={d.title}
      lines={d.lines}
      foot={d.foot}
      remember={
        d.remember === null
          ? null
          : { label: d.remember, checked: true, onChange: on.remember ?? (() => {}) }
      }
      confirm={d.confirm}
      cancel={d.cancel}
      primary={d.primary}
      onConfirm={on.confirm ?? (() => {})}
      onCancel={() => {}}
    />
  );
};

test("on the Mac the computer's alert asks in its own words, the box ticked, the default last", async () => {
  const onConfirm = vi.fn();
  await show(dialog({ kind: "folder", name: "Precedents" }, "mac", { confirm: onConfirm }));
  const os = one(".sysd-mac");
  expect(os.getAttribute("role")).toBe("alertdialog");
  expect(os.textContent).toContain("Allow Clauseo to read the folder 'Precedents'?");
  expect(os.textContent).toContain(
    "If you don't, Clauseo keeps to this matter's folder and answers without Precedents.",
  );
  // what is sent names the plan: it goes to Claude
  expect(os.textContent).toContain("what it reads goes to Claude as your papers do.");
  expect(one<HTMLInputElement>(".sysd-check-input").checked).toBe(true);
  const buttons = [...os.querySelectorAll(".sysd-button")].map((b) => b.textContent);
  expect(buttons).toEqual(["Don't allow", "Allow"]);
  expect(document.activeElement?.textContent).toBe("Allow");
  await press(button("Allow", os));
  expect(onConfirm).toHaveBeenCalledTimes(1);
});

test("on Windows the computer's dialog is a window of its own, Clauseo's title bar, the default button first", async () => {
  await show(dialog({ kind: "site", place: "example.org" }, "windows"));
  const os = one(".sysd-win");
  expect(one(".sysd-win-name", os).textContent).toBe("Clauseo");
  expect(os.textContent).toContain("Allow Clauseo to open example.org?");
  expect(os.querySelector(".sysd-button")?.textContent).toBe("Allow");
});

test("switching protection off: the computer asks, and keeping protection is the default", async () => {
  await show(dialog({ kind: "off", matter: "Acme Ltd v. State" }, "mac"));
  const os = one(".sysd-dialog");
  expect(os.textContent).toContain("Switch off protection for Acme Ltd v. State?");
  expect(os.querySelector(".sysd-button[data-primary]")?.textContent).toBe("Keep protection");
  expect(os.querySelector(".sysd-check-input")).toBeNull();
});

test("Not protected is explained once: three plain lines, and Understood, the dark primary, starts focused", async () => {
  await show(
    <NotProtectedNote
      open
      matter="Acme Ltd v. State"
      why="off"
      platform="mac"
      onClose={() => {}}
      onProtect={() => {}}
    />,
  );
  const note = one('[role="alertdialog"]');
  expect(note.textContent).toContain("This matter is not protected");
  expect(note.textContent).toContain("Protection is off for Acme Ltd v. State.");
  expect(note.textContent).not.toContain("The line under the question box");
  await vi.waitFor(() => expect(document.activeElement?.textContent).toBe("Understood"));
  expect(document.activeElement?.className).toContain("bg-primary");
});

test("the room's line says what the workspace does, or Not protected for as long as it lasts", async () => {
  await show(
    <>
      <WorkspaceLine open={false} onOpenChange={() => {}}>
        {null}
      </WorkspaceLine>
      <WorkspaceLine
        unprotected={{ why: "off", since: "5:12 pm" }}
        open={false}
        onOpenChange={() => {}}
      >
        {null}
      </WorkspaceLine>
    </>,
  );
  const lines = [...document.querySelectorAll(".ws-line")];
  expect(lines[0]?.textContent).toBe(
    "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.",
  );
  expect(lines[1]?.textContent).toContain("Not protected");
  expect(lines[0]?.textContent).not.toContain("Not protected");
});

test("what this matter allows: each place with Remove, and Remove can be undone", async () => {
  const onRemove = vi.fn();
  await show(
    <WorkspaceLine open onOpenChange={() => {}}>
      <AllowedList
        folder="~/Matters/Acme"
        allowed={[
          {
            id: "a",
            kind: "folder",
            place: "Documents/Precedents",
            note: "Read, allowed on 9 March",
          },
          {
            id: "b",
            kind: "site",
            place: "example.org",
            note: "Read and search, allowed at 5:12 pm today",
          },
        ]}
        refused={[
          {
            id: "c",
            kind: "site",
            place: "example.net",
            note: "Refused on 20 March",
            refused: true,
          },
        ]}
        onRemove={onRemove}
        onProtection={() => {}}
      />
    </WorkspaceLine>,
  );
  expect(words()).toContain("What this matter allows");
  expect(words()).toContain("~/Matters/Acme");
  const rows = [...document.querySelectorAll(".allows-row")].filter(
    (r) => r.querySelector(".allows-remove") !== null,
  );
  expect(rows.length).toBe(3);
  await press(one<HTMLElement>(".allows-remove", rows[0]));
  expect(onRemove).toHaveBeenCalledWith("a");
  expect(words()).toContain("Removed. Clauseo asks again before using it.");
  await press(one<HTMLElement>(".allows-undo"));
  expect(words()).not.toContain("Removed. Clauseo asks again before using it.");
});

test("after 3 refusals in a row, or 20 in all, Clauseo stops and asks", async () => {
  const refused = [
    { at: "5:01 pm", what: "Read Documents/Other matter" },
    { at: "5:02 pm", what: "Open example.net" },
  ];
  const onGoOn = vi.fn();
  await show(
    <RefusalPause
      count="row"
      refused={refused}
      earlier={0}
      state={{ kind: "pending" }}
      keys
      onGoOn={onGoOn}
      onStop={() => {}}
    />,
  );
  expect(one(".edge").textContent).toContain(
    "Its last 3 tries went beyond this matter and were refused.",
  );
  expect(document.querySelector(".edge-refused-earlier")).toBeNull();
  await press(button("Go on inside the matter"));
  expect(onGoOn).toHaveBeenCalledTimes(1);
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await show(
    <RefusalPause
      count="total"
      refused={refused}
      earlier={17}
      state={{ kind: "pending" }}
      onGoOn={() => {}}
      onStop={() => {}}
    />,
  );
  expect(one(".edge").textContent).toContain("20 tries in this answer");
  expect(one(".edge-refused-earlier").textContent).toContain("17 earlier");
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  await show(
    <RefusalPause
      count="row"
      refused={refused}
      earlier={0}
      state={{ kind: "goneOn" }}
      onGoOn={() => {}}
      onStop={() => {}}
    />,
  );
  expect(one(".perm-done").textContent).toBe("You told Clauseo to go on inside the matter.");
});
