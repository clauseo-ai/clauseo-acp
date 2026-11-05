// The pages a tool call's web lists: each address once, in the order they were opened.
import { expect, test } from "vitest";
import { pagesShown } from "./sources.ts";

test("the list shows each address once, where it first came, with the first title or a later one for none", () => {
  expect(
    pagesShown([
      { url: "https://a.example", via: "web_fetch" },
      { url: "https://b.example", title: "B", via: "web_fetch" },
      { url: "https://a.example", title: "A", via: "browser" },
      { url: "https://b.example", title: "B again", via: "web_fetch" },
    ]),
  ).toEqual([
    { url: "https://a.example", title: "A", via: "web_fetch" },
    { url: "https://b.example", title: "B", via: "web_fetch" },
  ]);
});

test("pages keep the order they were opened in", () => {
  expect(
    pagesShown([
      { url: "https://c.example", via: "web_fetch" },
      { url: "https://a.example", via: "web_fetch" },
      { url: "https://b.example", via: "web_fetch" },
    ]).map((p) => p.url),
  ).toEqual(["https://c.example", "https://a.example", "https://b.example"]);
});
