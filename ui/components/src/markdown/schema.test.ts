import type { Element, Root } from "hast";
import { sanitize } from "hast-util-sanitize";
import { expect, test } from "vitest";
import { SCHEMA } from "./schema.ts";

const element = (tagName: string, properties: Element["properties"]): Element => ({
  type: "element",
  tagName,
  properties,
  children: [{ type: "text", value: "x" }],
});

const table = (cell: Element): Element => ({
  type: "element",
  tagName: "table",
  properties: {},
  children: [{ type: "element", tagName: "tr", properties: {}, children: [cell] }],
});

test("the schema keeps no global attribute from GitHub's list (name, action, tabIndex, title…)", () => {
  const root: Root = {
    type: "root",
    children: [
      element("a", {
        href: "https://example.com",
        name: "n",
        tabIndex: 0,
        accessKey: ["k"],
        title: "t",
        useMap: "#m",
        dir: "rtl",
      }),
      element("p", { id: "p", lang: "en", align: "center", className: ["c"] }),
      element("img", { src: "clauseo-file:x", alt: "a", width: 9, longDesc: "https://x" }),
      table(element("td", { align: "left", colSpan: 9 })),
    ],
  };
  expect(sanitize(root, SCHEMA)).toEqual({
    type: "root",
    children: [
      element("a", { href: "https://example.com" }),
      element("p", {}),
      element("img", { src: "clauseo-file:x", alt: "a" }),
      table(element("td", { align: "left" })),
    ],
  });
});

test("tags our pipeline does not emit are dropped, their text kept", () => {
  const root: Root = {
    type: "root",
    children: ["form", "div", "span", "details", "b", "i", "ins", "kbd"].map((tag) =>
      element(tag, { action: "https://x", method: "post", encType: "text/plain" }),
    ),
  };
  expect(sanitize(root, SCHEMA)).toEqual({
    type: "root",
    children: Array.from({ length: 8 }, () => ({ type: "text", value: "x" })),
  });
});
