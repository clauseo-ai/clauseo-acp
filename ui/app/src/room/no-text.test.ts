// room/ holds no words of its own: every label comes from ui/components' words files. This parses each .tsx in room/
// and fails on any JSX text, or a string literal given as a label (aria-label, title, alt, placeholder, label).
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseSync } from "@babel/core";
import { expect, test } from "vitest";

const LABELS = new Set(["aria-label", "aria-description", "title", "alt", "placeholder", "label"]);

type Node = { type: string; [key: string]: unknown };
const isNode = (value: unknown): value is Node =>
  typeof value === "object" && value !== null && typeof (value as Node).type === "string";

/** The words a file's JSX says outright, each with its line. */
function jsxWords(source: string, file = "file.tsx"): string[] {
  const ast = parseSync(source, {
    filename: file,
    babelrc: false,
    configFile: false,
    parserOpts: { plugins: ["jsx", "typescript"] },
  });
  const found: string[] = [];
  const line = (node: Node) => (node["loc"] as { start: { line: number } } | undefined)?.start.line;
  const walk = (node: unknown): void => {
    if (Array.isArray(node)) {
      for (const child of node) walk(child);
      return;
    }
    if (!isNode(node)) return;
    if (node.type === "JSXText" && String(node["value"]).trim() !== "") {
      found.push(`${file}:${line(node)}: ${String(node["value"]).trim()}`);
    }
    if (node.type === "JSXExpressionContainer" && isNode(node["expression"])) {
      const e = node["expression"];
      if (e.type === "StringLiteral" || e.type === "TemplateLiteral") {
        found.push(`${file}:${line(node)}: a string as a child`);
      }
    }
    if (node.type === "JSXAttribute") {
      const name = (node["name"] as Node | undefined)?.["name"];
      const value = node["value"] as Node | null | undefined;
      const literal =
        value?.type === "StringLiteral" ||
        (value?.type === "JSXExpressionContainer" &&
          isNode(value["expression"]) &&
          ["StringLiteral", "TemplateLiteral"].includes(value["expression"].type));
      if (typeof name === "string" && LABELS.has(name) && literal && !isEmpty(value)) {
        found.push(`${file}:${line(node)}: ${name} as a literal`);
      }
      // the attribute's own value is no child text
      walk(value?.type === "JSXExpressionContainer" ? value["expression"] : null);
      return;
    }
    for (const [key, child] of Object.entries(node)) {
      if (key !== "loc" && key !== "start" && key !== "end") walk(child);
    }
  };
  walk(ast);
  return found;
}

const isEmpty = (value: Node | null | undefined) =>
  value?.type === "StringLiteral" && String(value["value"]) === "";

test("room/ holds no JSX text literal and no literal label", () => {
  const dir = dirname(fileURLToPath(import.meta.url));
  const files = readdirSync(dir).filter((f) => f.endsWith(".tsx") && !f.includes(".test."));
  expect(files).toContain("room.tsx");
  const found = files.flatMap((f) => jsxWords(readFileSync(join(dir, f), "utf8"), f));
  expect(found).toEqual([]);
});

test("the check finds words written into JSX, as text, as a child string and as a label", () => {
  const found = jsxWords(
    [
      "export const A = () => <div>",
      "  Safe workspace",
      '  <b>{"Papers"}</b>',
      '  <aside aria-label="Papers" title={`Hint`} className="room-pane" />',
      '  <h1 title="">{name}</h1>',
      "</div>;",
    ].join("\n"),
  );
  expect(found).toEqual([
    "file.tsx:1: Safe workspace",
    "file.tsx:3: a string as a child",
    "file.tsx:4: aria-label as a literal",
    "file.tsx:4: title as a literal",
  ]);
});
