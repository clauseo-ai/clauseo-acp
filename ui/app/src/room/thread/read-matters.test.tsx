// @vitest-environment happy-dom
// Letting this conversation read another matter's files: the refusal's offer under the host's other_matter_refused
// notice sends one allow_matter_read for this thread and that matter, and is gone once the header lists the matter
// (or from the start, when it does already); the composer's pill for each matter the header lists sends
// withdraw_matter_read from its ×.
import { afterEach, expect, test } from "vitest";
import { header, reset, THREAD, upsert, userMessage, uuid } from "../../testing/stream-script.ts";
import type { MatterSummary, ViewEntry } from "../../transport/protocol.ts";
import { mountRoom, THE_MATTER, type Harness } from "./harness.ts";

let h: Harness | null = null;
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const OTHER: MatterSummary = { matter: uuid(7, 2), name: "Joshi v. Joshi", kind: "folder" };
const GONE = uuid(8, 2);
const iso = (name: string) => `\u2068${name}\u2069`;

const refused = (matter: string, name: string): ViewEntry => ({
  notice: {
    order: 3,
    event: uuid(3, 10),
    level: "warning",
    text: "Clauseo kept the agent out of another matter's files.",
    ts: 3000,
    kind: { other_matter_refused: { matter, name } },
  },
});

/** The header, with the other matters the thread may read. */
const reading = (matters: string[]): ViewEntry => {
  const head = header();
  head.header!.read_matters = matters;
  return head;
};

const mount = () => mountRoom({ matters: [THE_MATTER, OTHER] });

test("the offer sends one allow_matter_read for this thread and that matter, then goes as the header lists it", async () => {
  h = await mount();
  h.deliver([reset([userMessage(1, 1, "Compare the two leases"), refused(OTHER.matter, "Joshi")])]);
  // the line names the matter as the app's list does now
  expect(h.one("notice")?.textContent).toBe(
    `Clauseo stopped a step: Claude tried to open ${iso("Joshi v. Joshi")}'s files. That's another matter.`,
  );
  const offer = h.one("read-offer");
  expect(offer?.textContent).toBe(`Let this conversation read ${iso("Joshi v. Joshi")}'s files`);
  await h.click(offer);
  expect(h.sent("allow_matter_read")).toEqual([{ thread: THREAD, matter: OTHER.matter }]);
  // the host logged it: the header lists the matter, the offer goes and the composer's pill comes
  h.deliver([upsert(reading([OTHER.matter]))]);
  await h.settle();
  expect(h.one("read-offer")).toBeNull();
  expect(h.one("read-chips")?.textContent).toBe(`Can read ${iso("Joshi v. Joshi")}'s files`);
  expect(h.sent("allow_matter_read")).toHaveLength(1);
});

test("no offer for a matter already allowed, or one the app's list no longer holds", async () => {
  h = await mount();
  h.deliver([
    reset(
      [userMessage(1, 1, "Compare the two leases"), refused(OTHER.matter, "Joshi")],
      reading([OTHER.matter]),
    ),
  ]);
  expect(h.one("notice")).not.toBeNull();
  expect(h.one("read-offer")).toBeNull();
  await h.unmount();
  h = await mount();
  h.deliver([reset([userMessage(1, 1, "Compare the two leases"), refused(GONE, "Kapoor")])]);
  expect(h.one("notice")?.textContent).toContain(iso("Kapoor"));
  expect(h.one("read-offer")).toBeNull();
});

test("the pill's × sends withdraw_matter_read, named for assistive technology", async () => {
  h = await mount();
  h.deliver([reset([userMessage(1, 1, "Compare the two leases")], reading([OTHER.matter]))]);
  await h.settle();
  const stop = h.one("read-chip-stop");
  expect(stop?.getAttribute("aria-label")).toBe(
    `Stop letting this conversation read ${iso("Joshi v. Joshi")}'s files`,
  );
  expect(h.one("read-chips")?.getAttribute("aria-label")).toBe(
    "Other matters this conversation can read",
  );
  await h.click(stop);
  expect(h.sent("withdraw_matter_read")).toEqual([{ thread: THREAD, matter: OTHER.matter }]);
  // the host logged it: the pill goes
  h.deliver([upsert(reading([]))]);
  await h.settle();
  expect(h.one("read-chips")).toBeNull();
});
