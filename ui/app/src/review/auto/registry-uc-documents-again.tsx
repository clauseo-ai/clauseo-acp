// UC's entry for RC A4 (UC-1550): the band's one quiet line when the desktop lists the Documents folder not asked again
// after an update (its answers are kept per build), on a Mac that answered it before. The real room over FakeTransport,
// a matter open, the keychain allowed, and this Mac's earlier answer remembered (documents-answered.ts).
import { useState, type ReactNode } from "react";
import { markDocumentsAnswered } from "../../room/first-run/documents-answered.ts";
import type { Entry } from "../entry.ts";
import { render } from "../registry-ud1-scene.tsx";
import { DOCS, KEYS, fakePermissions } from "../registry-uc-permissions.tsx";
import { firstLaunchWords as w } from "../words.ts";

/** Remembers the earlier answer before the room under it mounts and reads it. */
function Answered({ children }: { children: ReactNode }) {
  useState(markDocumentsAnswered);
  return children;
}

const scene = render({
  events: [
    { agent_install_progress: { agent: "claude-acp", state: { ready: { version: "0.1.0" } } } },
  ],
  permissions: fakePermissions([DOCS, { ...KEYS, state: "granted" }]),
});

const UC_DOCUMENTS_AGAIN: Entry[] = [
  {
    id: "UC-1550",
    words: w("Clauseo was updated. Allow it to open your Documents folder again."),
    screen: "room/thread",
    render: (lang) => <Answered>{scene(lang)}</Answered>,
  },
];

export default UC_DOCUMENTS_AGAIN;
