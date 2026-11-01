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
    children: ["form", "div", "span", "details", "b", "i", "font", "center"].map((tag) =>
      element(tag, { action: "https://x", method: "post", encType: "text/plain" }),
    ),
  };
  expect(sanitize(root, SCHEMA)).toEqual({
    type: "root",
    children: Array.from({ length: 8 }, () => ({ type: "text", value: "x" })),
  });
});

test("the approved inline tags and the syntax extras keep no attribute; a heading keeps only its id, prefixed", () => {
  const hostile = {
    id: "x",
    className: ["c"],
    style: "color:red",
    onClick: "alert(1)",
    title: "t",
    dataFootnotePending: "1",
  };
  const tags = [
    "sup",
    "sub",
    "u",
    "ins",
    "del",
    "s",
    "mark",
    "small",
    "kbd",
    "dl",
    "dt",
    "dd",
    "br",
  ];
  const root: Root = { type: "root", children: tags.map((tag) => element(tag, hostile)) };
  expect(sanitize(root, SCHEMA)).toEqual({
    type: "root",
    children: tags.map((tag) =>
      // A footnote reference still waiting for its note is a sup with this mark; the pipeline never puts
      // it on an inline tag (inline-tags.ts makes elements with no properties).
      element(tag, tag === "sup" ? { dataFootnotePending: "1" } : {}),
    ),
  });
  const heading: Root = { type: "root", children: [element("h2", hostile)] };
  expect(sanitize(heading, SCHEMA)).toEqual({
    type: "root",
    children: [element("h2", { id: "user-content-x" })],
  });
});
