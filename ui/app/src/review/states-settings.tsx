// The state U11's strings show in: the real settings window (src/settings/settings-app.tsx) over FakeTransport.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useState } from "react";
import { createDisplayStore, DEFAULT_UI } from "../room/display-store.ts";
import { nextHostOf, type MatterInfo } from "../room/host-next.ts";
import type { SectionId } from "../settings/sections.tsx";
import { SettingsApp } from "../settings/settings-app.tsx";
import type { FakeBlocklistHost } from "../transport/fake-blocklist.ts";
import { FakeTransport, type FakeProtectionHost } from "../transport/fake-transport.ts";
import type {
  ConnectorsState,
  LimitsView,
  SettingsView,
  SetupCommand,
  SetupEvent,
  ToolsSettingsView,
} from "../transport/protocol.ts";

/** The setup wire a cell's host carries (UD2's Plans): the events its feed starts with, after SetupInfo, and what
 * each command plays back, as desktop would answer it. */
export type CellSetup = {
  first: SetupEvent[];
  keys?: boolean;
  answer?: (command: SetupCommand) => SetupEvent[];
};

/** The window over a host holding the default settings in `lang`, with `locked` locked, and with `limits` the plan's
 * limits its channel carries (U12); the display stays the frame's own (the review sets it per cell), so the store
 * starts and shows nothing. `releaseDate` is the release date the host reports (null, a dev build, when left out). */
export function SettingsOver({
  section,
  lang,
  locked = [],
  limits,
  releaseDate = null,
  tools,
  connectors,
  setup,
  plans,
  protection,
  blocklist,
  matters = [],
}: {
  section: SectionId;
  lang: Lang;
  locked?: string[];
  limits?: LimitsView;
  releaseDate?: string | null;
  /** The tools' switches the host sends (SettingsView.tools, Privacy's switches); none when left out. */
  tools?: ToolsSettingsView;
  /** The connectors' state the app feed's catch-up carries (U15); none when left out. */
  connectors?: ConnectorsState;
  /** The setup wire (UD2's Plans); none when left out, and Plans doesn't show. */
  setup?: CellSetup;
  /** Each agent's firm plan the host sends (SettingsView.plans). */
  plans?: SettingsView["plans"];
  /** UC's protection wire; none when left out, and the Safe workspace doesn't show. */
  protection?: FakeProtectionHost;
  /** The unsafe-site list's host (0053), Privacy's allowed sites; none listed when left out. */
  blocklist?: FakeBlocklistHost;
  /** The matters the app feed's first frames list, for the allowed sites' matter names. */
  matters?: MatterInfo[];
}) {
  const [made] = useState(() => {
    const transport = new FakeTransport({
      releaseDate,
      next: {
        query: (query) => {
          if (query === "get_settings")
            return {
              settings: {
                ui: { ...DEFAULT_UI, language: lang },
                locked,
                ...(tools === undefined ? {} : { tools }),
                ...(plans === undefined ? {} : { plans }),
              },
            };
          throw new Error("not scripted");
        },
        first: () => [
          ...(connectors === undefined ? [] : [{ connectors_changed: connectors }]),
          ...(matters.length === 0
            ? []
            : [{ matters_changed: { matters, removed: [], all: true } }]),
        ],
      },
      ...(protection === undefined ? {} : { protection }),
      ...(blocklist === undefined ? {} : { blocklist }),
      ...(setup === undefined
        ? {}
        : {
            setup: {
              first: () => [{ setup_info: { keys_available: setup.keys ?? true } }, ...setup.first],
              command: (command: SetupCommand) => {
                const events = setup.answer?.(command) ?? [];
                queueMicrotask(() => {
                  for (const event of events) transport.setupEvent(event);
                });
              },
            },
          }),
    });
    if (limits !== undefined) transport.limitsChanged(limits);
    const store = createDisplayStore({
      storage: null,
      matchMedia: null,
      host: nextHostOf(transport).settings,
      start: () => {},
      show: () => {},
      override: { language: lang },
    });
    // what connectDisplay does with display_info's report, as the real window gets it
    store.setReleaseDate(releaseDate);
    return { transport, store };
  });
  return (
    <SettingsApp
      store={made.store}
      transport={made.transport}
      windowControls={null}
      section={section}
    />
  );
}
