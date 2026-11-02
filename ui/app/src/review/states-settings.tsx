// The state U11's strings show in: the real settings window (src/settings/settings-app.tsx) over FakeTransport.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { useState } from "react";
import { createDisplayStore, DEFAULT_UI } from "../room/display-store.ts";
import { nextHostOf } from "../room/host-next.ts";
import type { SectionId } from "../settings/sections.tsx";
import { SettingsApp } from "../settings/settings-app.tsx";
import { FakeTransport } from "../transport/fake-transport.ts";

/** The window over a host holding the default settings in `lang`, with `locked` locked; the display stays the
 * frame's own (the review sets it per cell), so the store starts and shows nothing. */
export function SettingsOver({
  section,
  lang,
  locked = [],
}: {
  section: SectionId;
  lang: Lang;
  locked?: string[];
}) {
  const [made] = useState(() => {
    const transport = new FakeTransport({
      next: {
        query: (query) => {
          if (query === "get_settings")
            return { settings: { ui: { ...DEFAULT_UI, language: lang }, locked } };
          throw new Error("not scripted");
        },
      },
    });
    const store = createDisplayStore({
      storage: null,
      matchMedia: null,
      host: nextHostOf(transport).settings,
      start: () => {},
      show: () => {},
      override: { language: lang },
    });
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
