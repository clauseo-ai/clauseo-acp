import { expect, test } from "vitest";
import { docId, docsSection } from "./running-docs.mjs";

const commit = "7198bc84a8e36b43ba961d6848e9745053b4d72b";
const DOCS = [
  {
    path: "docs/install/macos.md",
    title: "Installing Clauseo on a Mac",
    commit,
    by: "stage F (provisional; Rohan batch review)",
    pr: 442,
  },
  {
    path: "docs/install/windows.md",
    title: "Installing Clauseo on Windows",
    commit,
    by: "stage F (provisional; Rohan batch review)",
    pr: 442,
  },
];

test("no docs give no section", () => {
  expect(docsSection([])).toBe("");
});

test("each doc is an article with its GitHub blob and pull links", () => {
  const html = docsSection(DOCS);
  expect(html.startsWith('<h2 class="running-file">Docs</h2>')).toBe(true);
  expect(html.match(/<article /g)).toHaveLength(2);
  expect(html).toContain('<article class="review-item" id="doc-docs-install-windows-md">');
  expect(html).toContain('<article class="review-item" id="doc-docs-install-macos-md">');
  for (const d of DOCS) {
    expect(html).toContain(
      `href="https://github.com/clauseo-ai/clauseo-acp/blob/${commit}/${d.path}"`,
    );
  }
  expect(
    html.match(/href="https:\/\/github\.com\/clauseo-ai\/clauseo-acp\/pull\/442"/g),
  ).toHaveLength(2);
  expect(html).toContain("Written by stage F in");
  expect(html).not.toContain("Rohan batch review");
  expect(docId("docs/install/macos.md")).toBe("doc-docs-install-macos-md");
});

test("a title with < and & is escaped", () => {
  const html = docsSection([{ ...DOCS[0], title: 'Q&A <b>"x"</b>', by: "a <i>" }]);
  expect(html).toContain("<h2>Q&amp;A &lt;b&gt;&quot;x&quot;&lt;/b&gt;</h2>");
  expect(html).toContain("Written by a &lt;i&gt; in");
  expect(html).not.toContain("<b>");
});
