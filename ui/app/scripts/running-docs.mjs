// The Docs section of the running copy review page (copy-running.mjs): one article per provisional doc, with its title,
// its path, a link to the version under review on GitHub (the last commit that touched it), and who wrote it and in
// which pull request. Pure: copy-running.mjs reads ui/app/doc-approvals.json and git, and passes the entries in.

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
 * The Docs section for `entries` ({ path, title, commit, by, pr }, in order), or "" when there are none. `by` is who wrote
 * the doc with what it was added under in brackets, "stage F (provisional; Rohan batch review)"; only the part before
 * " (" is shown.
 */
export function docsSection(entries) {
  if (entries.length === 0) return "";
  const articles = entries.map((d) => {
    const who = String(d.by).split(" (")[0];
    return `<article class="review-item" id="${esc(docId(d.path))}"><header class="review-item-head"><h2>${esc(d.title)}</h2><span class="review-status" data-status="provisional">Provisional</span></header>
<p class="running-where"><code>${esc(d.path)}</code></p>
<p class="running-status"><a href="${esc(`${REPO}/blob/${d.commit}/${d.path}`)}">Read it on GitHub</a></p>
<p class="running-status">Written by ${esc(who)} in <a href="${esc(`${REPO}/pull/${d.pr}`)}">#${esc(d.pr)}</a>.</p>
</article>`;
  });
  return `<h2 class="running-file">Docs</h2>\n${articles.join("\n")}`;
}
