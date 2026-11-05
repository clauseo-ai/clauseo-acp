// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, test } from "vitest";
import { siteHost, siteLetter } from "./site-host.ts";
import { SiteTile } from "./site-tile.tsx";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement | null = null;
let root: Root | null = null;

async function mount(node: ReactNode) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root?.render(node));
  return host;
}

afterEach(async () => {
  await act(async () => root?.unmount());
  document.body.replaceChildren();
  root = null;
  host = null;
});

test("the domain's first letter, uppercase, with www. dropped", () => {
  expect(siteLetter("https://www.indiacode.nic.in/handle/1/")).toBe("I");
  expect(siteLetter("https://WWW.Example.com")).toBe("E");
  expect(siteLetter("http://user:pw@sci.gov.in:8080/x?y#z")).toBe("S");
  expect(siteLetter("livelaw.in/news")).toBe("L");
  expect(siteHost("https://www.bar.example.com/")).toBe("bar.example.com");
});

test("a Devanagari domain keeps its first letter whole, never punycode", () => {
  expect(siteHost("https://भारत.सरकार/पन्ना")).toBe("भारत.सरकार");
  expect(siteLetter("https://भारत.सरकार/पन्ना")).toBe("भा");
  expect(siteLetter("https://www.हिंदी.भारत")).toBe("हिं");
});

test("an address with no host has no letter", () => {
  for (const url of [
    "about:blank",
    "file:///Users/a/b.pdf",
    "data:text/html,x",
    "",
    "notes",
    "https:///x",
  ])
    expect(siteLetter(url), url).toBeNull();
});

test("the tile draws the letter, hidden, and never an image", async () => {
  const el = await mount(<SiteTile url="https://www.scconline.com/x" size={14} />);
  const tile = el.querySelector(".site-tile")!;
  expect(tile.textContent).toBe("S");
  expect(tile.getAttribute("aria-hidden")).toBe("true");
  expect(tile.getAttribute("data-size")).toBe("14");
  expect(tile.getAttribute("lang")).toBe("en");
  expect(el.querySelector("img")).toBeNull();
});

test("a Devanagari letter is set in Hindi's face", async () => {
  const el = await mount(<SiteTile url="https://भारत.सरकार" size={11} />);
  expect(el.querySelector(".site-tile")?.getAttribute("lang")).toBe("hi");
});

test("no host: the generic page glyph, still no image", async () => {
  const el = await mount(<SiteTile url="about:blank" size={11} />);
  const tile = el.querySelector(".site-tile")!;
  expect(tile.textContent).toBe("");
  expect(tile.querySelector("svg")).not.toBeNull();
  expect(el.querySelector("img")).toBeNull();
});
