// Settings, Privacy: the Permissions group (issue 5, #1303), at the pane's head, above what leaves this computer. A
// lawyer who already has matters never sees the first screen, so the first-start step's rows stand here too: the same
// rows, words and acts (room/first-run/permissions.tsx), Allow, Open System Settings, Check again, and on Windows the
// matters-folder choice where the host can set it. The group shows only while the desktop lists a row that needs the
// lawyer; with none, nothing. #1279 grants permissions() and ask_permission to the main window only: until the
// settings window is granted them too, the list fails here, reads as no rows, and the group never shows.
import { useState } from "react";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { permissionsHostOf } from "../../room/first-run/permissions-host.ts";
import { Permissions } from "../../room/first-run/permissions.tsx";
import type { SectionContext } from "../sections.tsx";
import { Group } from "./parts.tsx";

export function PermissionsGroup({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { transport } = ctx;
  const [host] = useState(() => permissionsHostOf(transport));
  return (
    <Permissions
      host={host}
      platform={ctx.display.platform}
      titled={false}
      onSettings={() => void transport.openPrivacySettings().catch(() => {})}
      wrap={(step) => (
        <div data-testid="settings-permissions">
          <Group title={w.s("Permissions")} plate={false}>
            {step}
          </Group>
        </div>
      )}
    />
  );
}
