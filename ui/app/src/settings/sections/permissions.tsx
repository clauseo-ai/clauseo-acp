// Settings, Privacy: the Permissions group (issue 5, #1303), at the pane's head, above what leaves this computer. A
// lawyer who already has matters never sees the first screen, so the first-start step's rows stand here too: the same
// rows, words and acts (room/first-run/permissions.tsx), Allow, Open System Settings, Check again, but never the
// matters-folder choice: the settings window may not pick a folder or send set_default_location (the desktop's
// capabilities/settings.json and bridge/admit.rs), so its host here has none, and no button that always fails. The
// group shows while the desktop lists a row, allowed or not; with none, nothing. The settings window is granted permissions() and ask_permission as the main window is
// (crates/desktop/capabilities/settings.json), so the rows are the desktop's own (transport/permission-commands.ts).
import { useState } from "react";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { permissionsHostOf, type PermissionsHost } from "../../room/first-run/permissions-host.ts";
import { Permissions } from "../../room/first-run/permissions.tsx";
import type { SectionContext } from "../sections.tsx";
import { Group } from "./parts.tsx";

export function PermissionsGroup({ ctx }: { ctx: SectionContext }) {
  const w = useSettingsWords();
  const { transport } = ctx;
  // the list and the ask only: no chooseMattersFolder in this window
  const [host] = useState((): PermissionsHost => {
    const desktop = permissionsHostOf(transport);
    return { list: () => desktop.list(), ask: (id) => desktop.ask(id) };
  });
  return (
    <Permissions
      host={host}
      platform={ctx.display.platform}
      heading={null}
      opens={(listed) => listed.length > 0}
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
