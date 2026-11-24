// @vitest-environment happy-dom
// Locate's refusal (matters.ts's locate, locate-refusal.ts): a folder the host refuses says why under the moved folder's
// line, in New matter's words for the code, and any other code in its generic words, never nothing; the lawyer picks
// again, which takes the refusal back, and the Locate that goes through is sent.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { isolate } from "@clauseo/components/lib/bidi.ts";
import { afterEach, expect, test } from "vitest";
import { createViewStore } from "../store/store.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import type { HostError, MatterId } from "../transport/protocol.ts";
import { uuid } from "../testing/stream-script.ts";
import { detailed, fakeNext } from "./fake-next.ts";
import type { NextCommand } from "./host-next.ts";
import { Matters } from "./matters.ts";
import { LocatedFolder } from "./moved.tsx";
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
const OTHER = detailed({ matter: uuid(3, 4), name: "Acme Traders", kind: "folder" });

/** The host's refusal, as the transport rejects it. */
const refusal = (code: string, matter?: MatterId): HostError =>
  ({
    code,
    message: "a host detail, for logs",
    retryable: false,
    ...(matter === undefined ? {} : { matter }),
  }) as HostError;

/** The moved folder's line as the room mounts it: LocatedFolder over the open matter, Locate folder sending locate. */
function Moved({ matters, matter }: { matters: Matters; matter: MatterId }) {
  return <LocatedFolder matters={matters} onLocate={() => void matters.locate(matter)} />;
}

/** The room's matters over a fake host whose relocate `refuse` may reject, with the picker's folders in turn. */
async function mount(
  refuse: (folder: string) => HostError | undefined,
  picks: readonly { folder: string; display: string }[] = [
    { folder: "ref-bad", display: "Documents/Acme/Inside" },
  ],
) {
  const sent: NextCommand[] = [];
  let picked = 0;
  const transport = new FakeTransport({
    pickFolder: () => picks[Math.min(picked++, picks.length - 1)] ?? null,
    next: fakeNext({
      matters: () => [MOVED, OTHER],
      command: (c) => {
        sent.push(c);
        if ("relocate_matter" in c) {
          const error = refuse(c.relocate_matter.folder);
          if (error !== undefined) throw error;
        }
      },
    }),
  });
  const matters = new Matters(
    transport,
    createViewStore(transport, { batch: () => () => () => {} }),
  );
  const stop = matters.start();
  await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  const open = matters.view().open;
  if (open === null) throw new Error("no open matter");
  connectRoom(transport, { language: "en" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(<Moved matters={matters} matter={open.matter} />));
  const press = async () => {
    await act(async () => host?.querySelector("button")?.click());
    await act(async () => new Promise((resolve) => setTimeout(resolve, 0)));
  };
  const alert = () => host?.querySelector('[role="alert"]')?.textContent ?? null;
  return { matters, transport, sent, stop, press, alert, open };
}

test.each([
  "matter.folder_inside_matter",
  "matter.folder_holds_matter",
  "matter.folder_is_matter",
  // what the host's relocate check sends for a folder inside another matter (matter_worker/relocate.rs's Registry::check)
  "matter.folder_refused",
])("a Locate refused as %s says New matter's words for it, under the moved line", async (code) => {
  const { stop, press, alert, sent, open } = await mount(() => refusal(code));
  expect(alert()).toBeNull();
  await press();
  expect(sent).toContainEqual({ relocate_matter: { matter: open.matter, folder: "ref-bad" } });
  expect(alert()).toBe("Clauseo can't make a matter of this folder. Choose another folder.");
  expect(host?.querySelector('[role="alert"]')?.className).toBe("text-small-row text-primary");
  expect(host?.textContent).toContain("This matter's folder has moved.");
  stop();
});

test.each([
  ["matter.folder_inside_matter", "This folder is inside the folder of "],
  ["matter.folder_holds_matter", "This folder holds the folder of "],
])("a %s naming a matter on the list says it, as New matter does", async (code, before) => {
  const { stop, press, alert } = await mount(() => refusal(code, OTHER.matter));
  await press();
  expect(alert()).toBe(`${before}${isolate("Acme Traders")}.`);
  stop();
});

test.each([
  ["an unknown code", "matter.something_new"],
  ["an unknown folder code", "matter.folder_new_rule"],
])("%s says the generic words, not nothing", async (_, code) => {
  const { stop, press, alert } = await mount(() => refusal(code));
  await press();
  expect(alert()).toBe("Something went wrong inside Clauseo.");
  stop();
});

test("another known code says its own words from words-errors", async () => {
  const { stop, press, alert } = await mount(() => refusal("matter.turn_running"));
  await press();
  expect(alert()).toBe("Wait for the answers in this matter to finish.");
  stop();
});

test("after a refused Locate the lawyer can pick again: the refusal goes, and the Locate that goes through is sent", async () => {
  const { stop, press, alert, sent, transport, open } = await mount(
    (folder) => (folder === "ref-bad" ? refusal("matter.folder_inside_matter") : undefined),
    [
      { folder: "ref-bad", display: "Documents/Acme/Inside" },
      { folder: "ref-ok", display: "Documents/Khan" },
    ],
  );
  await press();
  expect(alert()).toBe("Clauseo can't make a matter of this folder. Choose another folder.");
  const button = host?.querySelector("button");
  expect(button?.disabled).toBe(false);
  await press();
  expect(transport.only("pick_folder")).toHaveLength(2);
  expect(alert()).toBeNull();
  expect(sent.filter((c) => "relocate_matter" in c)).toEqual([
    { relocate_matter: { matter: open.matter, folder: "ref-bad" } },
    { relocate_matter: { matter: open.matter, folder: "ref-ok" } },
  ]);
  stop();
});
