import { expect, test } from "vitest";
import { docId, docsSection, nsisLines, nsisText } from "./running-docs.mjs";

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

const NSH = [
  "; a comment that mentions LangString x",
  'LangString deleteAppData ${LANG_ENGLISH} "Also delete conversations"',
  'LangString deleteAppDataKept ${LANG_ENGLISH} "Kept: $R8 and it said "quoted"$\\n$\\nDone"',
  'LangString deleteAppDataKeptFailed ${LANG_ENGLISH} "Line one$\\nLine two <b> & $R9"',
].join("\n");
const UNINSTALL_LINES = [
  { key: "deleteAppDataKept", text: 'Kept: $R8 and it said "quoted"$\\n$\\nDone' },
];
const SLOTS = { $R9: "the data folder", $R8: "the folder it kept" };

test("nsisLines finds each key's English text, in the order given", () => {
  expect(nsisLines(NSH, ["deleteAppDataKeptFailed", "deleteAppData"])).toEqual([
    { key: "deleteAppDataKeptFailed", text: "Line one$\\nLine two <b> & $R9" },
    { key: "deleteAppData", text: "Also delete conversations" },
  ]);
});

test("nsisLines reads the text to the last quote, so quotes and $\\n inside stay", () => {
  expect(nsisLines(NSH, ["deleteAppDataKept"])).toEqual(UNINSTALL_LINES);
});

test("nsisLines throws on a missing key, and on a key with no English line", () => {
  expect(() => nsisLines(NSH, ["deleteAppDataFailed"])).toThrow(/deleteAppDataFailed/);
  expect(() => nsisLines(NSH, ["x"])).toThrow(/no English LangString "x"/);
  expect(() => nsisLines('LangString k ${LANG_ENGLISH} "open', ["k"])).toThrow(/"k"/);
});

test("nsisText turns $\\n into breaks and names each slot", () => {
  expect(nsisText("A$\\n$\\nB $R9.", SLOTS)).toBe(
    'A<br><br>B <span class="running-slot">{the data folder}</span>.',
  );
  expect(nsisText("$R8$R9", SLOTS)).toBe(
    '<span class="running-slot">{the folder it kept}</span><span class="running-slot">{the data folder}</span>',
  );
});

test("nsisText escapes the rest before the markup goes in", () => {
  expect(nsisText('<b>&</b> "q" $R9', SLOTS)).toBe(
    '&lt;b&gt;&amp;&lt;/b&gt; &quot;q&quot; <span class="running-slot">{the data folder}</span>',
  );
  expect(nsisText("x$\\ny", {})).toBe("x<br>y");
});

test("an entry with lines lists each key and its text after the path", () => {
  const html = docsSection([
    {
      path: "crates/desktop/windows/nsis/English.nsh",
      title: "Messages the Windows uninstaller shows",
      commit,
      by: "stage F (provisional; Rohan batch review)",
      pr: 473,
      lines: [
        { key: "deleteAppDataFailed", text: "Could not delete.$\\n$\\n$R9" },
        { key: "deleteAppDataKept", text: "Kept: $R8 & <i>" },
      ],
      slots: SLOTS,
    },
  ]);
  expect(html).toContain("<h2>Messages the Windows uninstaller shows</h2>");
  expect(html).toContain(
    '<ul class="running-lines">\n' +
      "<li><code>deleteAppDataFailed</code>" +
      '<p class="running-words" lang="en">Could not delete.<br><br><span class="running-slot">{the data folder}</span></p></li>\n' +
      "<li><code>deleteAppDataKept</code>" +
      '<p class="running-words" lang="en">Kept: <span class="running-slot">{the folder it kept}</span> &amp; &lt;i&gt;</p></li>\n' +
      "</ul>",
  );
  expect(html.indexOf("running-lines")).toBeGreaterThan(html.indexOf("running-where"));
  expect(html.indexOf("running-lines")).toBeLessThan(html.indexOf("Read it on GitHub"));
  expect(html).not.toContain("<i>");
});

test("an entry with no lines renders as it did, with no list", () => {
  expect(docsSection([{ ...DOCS[0], lines: [] }])).toBe(docsSection([DOCS[0]]));
  expect(docsSection([DOCS[0]])).not.toContain("running-lines");
});
