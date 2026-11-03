// @vitest-environment happy-dom
// matters/moved's parts for U3's Work: the moved folder's line and the composer's gate, each with Locate folder, which
// sends matters.ts's locate (the picker, then relocate_matter) for the open matter.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { mt } from "@clauseo/components/lib/words-matters.ts";
import { afterEach, expect, test } from "vitest";
import { createViewStore } from "../store/store.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { NextCommand } from "./host-next.ts";
import { Matters } from "./matters.ts";
import { IconLocateFolder } from "@clauseo/components/icons/icons.tsx";
import { iconIn } from "@clauseo/components/icons/icon-in.tsx";
import { LocateGate, MovedFolder } from "./moved.tsx";
import { connectRoom } from "./start.ts";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const MOVED = detailed(
  { matter: uuid(1, 2), name: "Khan Leather Works v. Customs", kind: "folder" },
  { folder_state: "missing", folder_display: "Documents/Khan" },
);

async function render(node: React.ReactNode, transport = new FakeTransport()) {
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

test("the moved folder's line says where it was, that nothing is lost, and Locate folder relocates the open matter", async () => {
  const sent: NextCommand[] = [];
  const transport = new FakeTransport({
    pickFolder: () => ({ folder: "ref-found", display: "Desktop/Khan" }),
    next: fakeNext({ matters: () => [MOVED], command: (c) => void sent.push(c) }),
  });
  const matters = new Matters(
    transport,
    createViewStore(transport, { batch: () => () => () => {} }),
  );
  const stop = matters.start();
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  const view = matters.view();
  expect(view.folderMissing).toBe(true);
  const open = view.open;
  if (open === null) throw new Error("no open matter");
  await render(
    <MovedFolder
      {...(open.folder_display === undefined ? {} : { folder: open.folder_display })}
      onLocate={() => void matters.locate(open.matter)}
    />,
    transport,
  );
  const text = host?.textContent ?? "";
  expect(text).toContain(mt("en", "This matter's folder has moved."));
  // the path isolated, FSI before and PDI after
  expect(text).toContain("It was at \u2068Documents/Khan\u2069.");
  expect(text).toContain("nothing is lost while the folder is away");
  expect(host?.querySelector('[role="status"]')).not.toBeNull();
  await act(async () => host?.querySelector("button")?.click());
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  expect(transport.only("pick_folder")).toHaveLength(1);
  expect(sent).toContainEqual({ relocate_matter: { matter: MOVED.matter, folder: "ref-found" } });
  stop();
});

test("without a folder to name, the line leaves out where it was", async () => {
  await render(<MovedFolder onLocate={() => {}} />);
  expect(host?.textContent).not.toContain("It was at");
  expect(host?.textContent).toContain(mt("en", "This matter's folder has moved."));
});

test("the composer's gate says why it can't ask, with Locate folder", async () => {
  let pressed = 0;
  await render(<LocateGate onLocate={() => (pressed += 1)} />);
  expect(host?.textContent).toContain(mt("en", "Locate the folder to ask about this matter"));
  await act(async () => host?.querySelector("button")?.click());
  expect(pressed).toBe(1);
});

test.each([
  ["the moved folder's line", <MovedFolder key="line" onLocate={() => {}} />],
  ["the composer's gate", <LocateGate key="gate" onLocate={() => {}} />],
])(
  "%s: Locate folder leads with the folder-search icon, hidden, as the matter's menu does",
  async (_, node) => {
    await render(node);
    const button = host?.querySelector("button");
    const svg = iconIn(button, IconLocateFolder, 16);
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(button?.firstElementChild).toBe(svg);
    expect(button?.textContent).toBe("Locate folder");
  },
);
