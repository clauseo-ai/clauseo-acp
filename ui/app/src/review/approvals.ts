// The copy approvals, for the page and the gate: every ui/app/copy-approvals/*.json (one file per brief, named by the
// id's prefix: U1.json, UTC.json), each checked, merged into one map. An id in two files fails the build.
import { mergeApprovals } from "./status.ts";

export const APPROVALS = mergeApprovals(
  import.meta.glob<unknown>("../../copy-approvals/*.json", { eager: true, import: "default" }),
);
