// The Docs section of the running copy review page (copy-running.mjs): one article per provisional doc, with its title,
// its path, a link to the version under review on GitHub (the last commit that touched it), and who wrote it and in
// which pull request. A doc entry can also list NSIS LangStrings (`lines`, keys in an .nsh file), each shown with its
// English text inline, "$\n" as a line break and each slot ($R9 and the like) named in words. Pure: copy-running.mjs
// reads ui/app/doc-approvals.json, the files and git, and passes the entries in.

const REPO = "https://github.com/clauseo-ai/clauseo-acp";

/** Text as HTML: the five characters that can open a tag, an entity or an attribute value. */
export const esc = (s) =>
  String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");

/** An article's id: "doc-" and the doc's path with / and . as -, so docs/install/windows.md is doc-docs-install-windows-md. */
export const docId = (path) => `doc-${path.replaceAll("/", "-").replaceAll(".", "-")}`;

/**
 * The English text of each NSIS LangString in `source` (the text of an .nsh file) named in `keys`, in the order given:
 * [{ key, text }]. A string is one line, `LangString <key> ${LANG_ENGLISH} "<text>"`; its text runs up to the last `"`
 * on that line, so it may hold quotes of its own. Throws when a key has no such line.
 */
export function nsisLines(source, keys) {
  const found = new Map();
  for (const line of source.split(/\r?\n/)) {
    const m = /^LangString (\S+) \$\{LANG_ENGLISH\} "/.exec(line);
    if (m && !found.has(m[1])) {
      const rest = line.slice(m[0].length);
      const end = rest.lastIndexOf('"');
      if (end !== -1) found.set(m[1], rest.slice(0, end));
    }
  }
  return keys.map((key) => {
    if (!found.has(key))
      throw new Error(`no English LangString "${key}" (one line, LANG_ENGLISH) in the file`);
    return { key, text: found.get(key) };
  });
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * A LangString's English text as HTML, as the lawyer reads it: "$\n" is a line break, each slot key in `slots` (a map
 * from register to what it holds, "the data folder") is "{the data folder}" in a running-slot span, and everything else
 * is escaped before any markup goes in.
 */
export function nsisText(text, slots = {}) {
  const slotKeys = Object.keys(slots).sort((a, b) => b.length - a.length);
  const tokens = [escapeRe("$\\n"), ...slotKeys.map(escapeRe)];
  return text
    .split(new RegExp(`(${tokens.join("|")})`))
    .map((part) => {
      if (part === "") return "";
      if (part === "$\\n") return "<br>";
      if (Object.hasOwn(slots, part))
        return `<span class="running-slot">{${esc(slots[part])}}</span>`;
      return esc(part);
    })
    .join("");
}

/** The inline list of a doc's LangStrings: each key in <code>, its English text under it. */
function linesList(lines, slots) {
  const items = lines.map(
    ({ key, text }) =>
      `<li><code>${esc(key)}</code><p class="running-words" lang="en">${nsisText(text, slots)}</p></li>`,
  );
  return `<ul class="running-lines">\n${items.join("\n")}\n</ul>`;
}

/**
 * The Docs section for `entries` ({ path, title, commit, by, pr, lines?, slots? }, in order), or "" when there are none.
 * `by` is who wrote the doc with what it was added under in brackets, "stage F (provisional; Rohan batch review)"; only
 * the part before " (" is shown. `lines` ([{ key, text }], from nsisLines) and `slots` ({ register: what it holds }) add
 * the inline list of LangStrings after the path, when there are any.
 */
export function docsSection(entries) {
  if (entries.length === 0) return "";
  const articles = entries.map((d) => {
    const who = String(d.by).split(" (")[0];
    const lines = d.lines?.length > 0 ? `\n${linesList(d.lines, d.slots)}` : "";
    return `<article class="review-item" id="${esc(docId(d.path))}"><header class="review-item-head"><h2>${esc(d.title)}</h2><span class="review-status" data-status="provisional">Provisional</span></header>
<p class="running-where"><code>${esc(d.path)}</code></p>${lines}
<p class="running-status"><a href="${esc(`${REPO}/blob/${d.commit}/${d.path}`)}">Read it on GitHub</a></p>
<p class="running-status">Written by ${esc(who)} in <a href="${esc(`${REPO}/pull/${d.pr}`)}">#${esc(d.pr)}</a>.</p>
</article>`;
  });
  return `<h2 class="running-file">Docs</h2>\n${articles.join("\n")}`;
}
