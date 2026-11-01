// @vitest-environment happy-dom
// EmptyPapers' additive props: body false leaves its second sentence out, and without onAdd there is no Add papers.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { t } from "../lib/words.ts";
import { EmptyPapers } from "./empty.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: React.ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const SECOND = t(
  "en",
  "Add the matter's papers and Clauseo reads them before your first question.",
);

test("by default it says what goes here and what Clauseo does with it, with Add papers", async () => {
  let added = 0;
  await mount(<EmptyPapers onAdd={() => (added += 1)} />);
  expect(host?.textContent).toContain(t("en", "No papers yet."));
  expect(host?.textContent).toContain(SECOND);
  const add = host?.querySelector("button");
  expect(add?.textContent).toBe(t("en", "Add papers"));
  await act(async () => add?.click());
  expect(added).toBe(1);
});

test("body false leaves the second sentence out and keeps the first", async () => {
  await mount(<EmptyPapers body={false} onAdd={() => {}} />);
  expect(host?.textContent).toContain(t("en", "No papers yet."));
  expect(host?.textContent).not.toContain(SECOND);
  expect(host?.querySelector(".empty-body")).toBeNull();
});

test("without onAdd there is no Add papers button", async () => {
  await mount(<EmptyPapers body={false} />);
  expect(host?.querySelector("button")).toBeNull();
  expect(host?.textContent).toBe(t("en", "No papers yet."));
});
