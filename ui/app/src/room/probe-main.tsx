// The probe build's page (vite.config.ts's probe mode, `pnpm build:probe`): the room over FakeTransport, with one
// matter, its papers, and the scripted stream (testing/stream-script.ts) played to whatever subscribes. The page's
// query sets its display for this page only (theme=night, lang=hi, lite=1, motion=reduced, size=18), as the WebKit
// probe (scripts/webkit-probe.swift) asks for it, and window.__clauseoDisplay and __clauseoAxe are set (dev-hooks.ts).
// Not shipped.
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App.tsx";
import type { HostEvent, ViewPatchBody } from "../transport/protocol.ts";
import { FakeTransport } from "../transport/fake-transport.ts";
import {
  batch,
  event,
  MATTER,
  reset,
  scriptedTurn,
  uuid,
  userMessage,
} from "../testing/stream-script.ts";
import { installDevHooks, queryPatch } from "./dev-hooks.ts";
import { DEFAULT_UI } from "./display-store.ts";
import type { PaperSummary } from "./host-next.ts";
import { connectRoom } from "./start.ts";

const FILES = [
  "Notice to the bank.pdf",
  "Reply to the notice.docx",
  "Board minutes, 2 March 2001.pdf",
  "Complaint under section 138.docx",
  "Bank statement, March 2001.pdf",
];

const papers: PaperSummary[] = FILES.map((name, i) => ({
  file: uuid(i + 1, 8),
  path: `papers/${name}`,
  size: 40_000 + i * 1_000,
  modified: 1_730_380_000_000 + i * 60_000,
}));

/** The scripted turn after the prompt, in batches of 25 bodies, as the bridge would deliver them. */
function stream(): HostEvent[] {
  const bodies: ViewPatchBody[] = [
    reset([userMessage(2, 1, "When did the bank reply?")]),
    ...scriptedTurn().bodies,
  ];
  const events: HostEvent[] = [];
  for (let at = 0; at < bodies.length; at += 25) {
    events.push(event(batch(at + 1, bodies.slice(at, at + 25))));
  }
  return events;
}

const transport = new FakeTransport({
  platform: "mac",
  query: (query) => {
    if (query === "list_matters") {
      return { matters: [{ matter: MATTER, name: "Sharma v. State of Delhi", kind: "folder" }] };
    }
    throw new Error(`the probe's host answers list_matters only, not ${JSON.stringify(query)}`);
  },
  subscribe: () => {
    queueMicrotask(() => {
      for (const message of stream()) transport.deliver(message);
    });
  },
  next: {
    query: (query) =>
      query === "get_settings"
        ? { settings: { ui: DEFAULT_UI, locked: [] } }
        : { papers: { papers, skipped: [], truncated: false } },
  },
});

// the page's query over the settings, before the first render, so night never starts as day; and the hooks the
// by-hand checks use, so the WebKit probe can run window.__clauseoAxe() as it would run in the app
installDevHooks(connectRoom(transport, queryPatch(location.search)));

const container = document.getElementById("root");
if (container === null) throw new Error("index.html has no #root element");
createRoot(container).render(
  <StrictMode>
    <App transport={transport} />
  </StrictMode>,
);
