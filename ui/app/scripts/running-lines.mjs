// Which approvals the running copy review page (copy-running.mjs) lists, and how each line says where its English and
// its Hindi stand. A line is listed while its English is provisional ("status": "provisional") or its Hindi is
// ("hi_status": "provisional", with hi_at and hi_by): an approved English with a provisional Hindi is shown as
// approved, its Hindi marked provisional. Pure: copy-running.mjs reads and checks the approvals and passes them in.
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
