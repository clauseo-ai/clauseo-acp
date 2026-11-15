// Which approvals the running copy review page (copy-running.mjs) lists, and how each line says where its English and
// its Hindi stand. A line is listed while its English is provisional ("status": "provisional") or its Hindi is
// ("hi_status": "provisional", with hi_at and hi_by): an approved English with a provisional Hindi is shown as
// approved, its Hindi marked provisional. With --english, the page lists only the English not yet approved (provisional
// or a change asked for), grouped by the area of the screen that shows it. Pure: copy-running.mjs reads and checks the
// approvals and passes them in.
import { esc } from "./running-docs.mjs";

/** Whether an approval is on the running page: its English or its Hindi is provisional. */
export const onRunningPage = (a) => a.status === "provisional" || a.hi_status === "provisional";

const ENGLISH = { approved: "approved", change: "change requested" };

/** A listed line's badge, and its status line as HTML: the English's state, then the Hindi's. */
export function lineStatus(a) {
  const by = a.by === undefined ? "" : ` by ${esc(a.by)}`;
  const english =
    a.status === "provisional"
      ? `Added ${esc(a.at)}${by}.`
      : `English ${ENGLISH[a.status] ?? esc(a.status)} ${esc(a.at)}${by}.`;
  const hindi =
    a.hi === null
      ? "Hindi: later."
      : a.hi_status === "provisional"
        ? `Hindi (provisional): <span lang="hi">${esc(a.hi)}</span>. Added ${esc(a.hi_at)} by ${esc(a.hi_by)}.`
        : `Hindi: <span lang="hi">${esc(a.hi)}</span>.`;
  return {
    badge: a.status === "provisional" ? "Provisional" : "Hindi provisional",
    html: `${english} <span class="running-hi">${hindi}</span>`,
  };
}

/** Whether an approval is on the English page (copy-running.mjs --english): its English is not approved yet. */
export const onEnglishPage = (a) => a.status !== "approved";

/** The English page's group for a line: the first part of its registry screen ("room/thread" is "room"). */
export const areaOf = (entry) =>
  entry === undefined ? "not in the review registry" : entry.screen.split("/")[0];

/** The English page's lines grouped by area, areas by name, each area's lines by screen then in file order. */
export function byArea(lines) {
  const areas = new Map();
  for (const l of lines) {
    const area = areaOf(l.entry);
    if (!areas.has(area)) areas.set(area, []);
    areas.get(area).push(l);
  }
  return [...areas]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([area, ls]) => ({
      area,
      lines: ls
        .map((l, i) => ({ l, i }))
        .sort(
          (x, y) => (x.l.entry?.screen ?? "").localeCompare(y.l.entry?.screen ?? "") || x.i - y.i,
        )
        .map(({ l }) => l),
    }));
}

/** A line's badge and status line on the English page: the English's state and when it was added; no Hindi. */
export function englishStatus(a) {
  const by = a.by === undefined ? "" : ` by ${esc(a.by)}`;
  return a.status === "change"
    ? { badge: "Change asked for", html: `Change asked for ${esc(a.at)}${by}.` }
    : { badge: "Provisional", html: `Added ${esc(a.at)}${by}.` };
}
