// copy-approvals.json, checked, for the page and the gate.
import json from "../../copy-approvals.json" with { type: "json" };
import { parseApprovals } from "./status.ts";

export const APPROVALS = parseApprovals(json);
