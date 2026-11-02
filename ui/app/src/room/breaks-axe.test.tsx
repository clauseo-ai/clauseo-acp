// @vitest-environment happy-dom
// axe-core over the room in each of U10's states, in both themes and languages: Claude stopped mid-answer (before H10,
// and with H10 faked, Restart's details open), a stream that failed with Try again, and a reopened thread's mark.
// happy-dom computes no styles, so colour contrast is checked by hand in the app (window.__clauseoAxe()).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import axe from "axe-core";
import { settled } from "@clauseo/components/lib/late.ts";
import { act } from "react";
import { afterEach, beforeAll, expect, test } from "vitest";
import {
  agentText,
  header,
  reset,
  step,
  STREAM,
  upsert,
  userMessage,
} from "../testing/stream-script.ts";
import type { ViewEntry } from "../transport/protocol.ts";
import { mountRoom, type Harness, type Script } from "./thread/harness.ts";

let h: Harness | null = null;
beforeAll(() => {
  // the page's own title, as index.html gives it
  const html = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../../index.html"),
    "utf8",
  );
  document.title = /<title>([^<]*)<\/title>/.exec(html)?.[1] ?? "";
});
afterEach(async () => {
  await h?.unmount();
  h = null;
  document.body.replaceChildren();
});

const status = (s: "running" | "agent_exited"): ViewEntry => {
  const head = header();
  head.header!.status = s;
  return head;
};
const ASKED = userMessage(5, 1, "Summarise the notice to the bank.");
const SO_FAR = agentText(6, 1, "Clause nine requires notice");
const EXITED = step(9, { stop: "failed", failure: "agent.exited", ts: Date.now() });

type State = { next?: Script["next"]; run: (room: Harness) => Promise<void>; expect: string };

const live = async (room: Harness) => {
  room.deliver([reset([ASKED, SO_FAR], status("running"))]);
  room.deliver([upsert(EXITED), upsert(status("agent_exited"))]);
  await room.settle();
};

const STATES: Record<string, State> = {
  "stopped, before H10": { run: live, expect: "agent-stopped" },
  "stopped, with H10 and its details open": {
    next: { parts: ["restart"], query: () => ({ threads: [] }) },
    run: async (room) => {
      await live(room);
      await act(settled);
      const more = room.host.querySelector<HTMLElement>(".working-more");
      await room.click(more);
      await act(settled);
    },
    expect: "restart",
  },
  "a stream that failed": {
    run: async (room) => {
      room.deliver([reset([ASKED, SO_FAR])]);
      await act(async () =>
        room.transport.failStream(STREAM, { code: "store.io", message: "x", retryable: true }),
      );
    },
    expect: "retry",
  },
  "a reopened thread's mark": {
    run: async (room) => {
      room.deliver([reset([ASKED, SO_FAR, EXITED], status("agent_exited"))]);
      await room.settle();
    },
    expect: "stopped-here",
  },
};

for (const theme of ["light", "dark"] as const) {
  for (const language of ["en", "hi"] as const) {
    test(`axe finds no violation in U10's states: ${theme}, ${language}`, async () => {
      for (const [name, state] of Object.entries(STATES)) {
        h = await mountRoom({
          display: { theme, language },
          ...(state.next === undefined ? {} : { next: state.next }),
        });
        await state.run(h);
        expect(h.one(state.expect), name).not.toBeNull();
        const results = await axe.run(document, { resultTypes: ["violations"] });
        const found = results.violations.map(
          (v) => `${name}: ${v.id} at ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`,
        );
        expect(found).toEqual([]);
        await h.unmount();
        h = null;
      }
    }, 20_000);
  }
}
