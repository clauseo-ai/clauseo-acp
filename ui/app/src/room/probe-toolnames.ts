// ?thread=permission&tools=raw for the probe build: the turn's permission requests carry only a tool's or a
// permission's own name, as an agent sometimes sends it (thread/tool-names.ts): a step titled "SandboxNetworkAccess"
// that went through, then a pending prompt for another. Not shipped: only the probe build imports it.
import { permission, TURN, upsert, uuid } from "../testing/stream-script.ts";
import type { ToolCallStatus, ViewPatchBody } from "../transport/protocol.ts";

const named = (order: number, n: number, status: ToolCallStatus): ViewPatchBody =>
  upsert({
    tool_call: {
      order,
      tool_call: uuid(n, 7),
      turn: TURN,
      started_at: order * 1000,
      title: "SandboxNetworkAccess",
      kind: "other",
      status,
      locations: [],
      content: [],
    },
  });

/** The requests on ?tools=raw, in place of the page's own: a done step, then a pending prompt, both by name only. */
export const RAW_TOOL_PERMISSIONS: ViewPatchBody[] = [
  named(100, 311, "completed"),
  named(102, 312, "pending"),
  upsert(permission(103, 312)),
];

/** Whether the page asks for the raw names. */
export const rawTools = (page: URLSearchParams): boolean => page.get("tools") === "raw";
