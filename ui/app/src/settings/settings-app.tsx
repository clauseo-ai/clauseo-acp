// The settings window's page: the frame (ui/components' SettingsWindow) over the sections built so far (sections.tsx),
// each control one patch to the display store, which both windows share. The store says when the settings or the
// locks change, from this window, the other, or the host, and the controls follow; the window's own caption buttons
// are wired off the Mac, as the room's are.
import { SettingsWindow } from "@clauseo/components/clauseo/settings-window.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { useEffect, useMemo, useState } from "react";
import type { DisplayStore } from "../room/display-store.ts";
import { LimitsContext, limitsStoreOf } from "../store/limits.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { tauriWindowControls, type WindowControls } from "../transport/window-controls.ts";
import { setWindowTitle } from "../transport/window-title.ts";
import { useWindowFrame } from "../window/use-window-frame.ts";
import { contextOf, sectionOf, shownSections, type SectionId } from "./sections.tsx";
import { useSettings } from "./use-settings.ts";

export function SettingsApp({
  store,
  transport,
  windowControls,
  section,
}: {
  store: DisplayStore;
  transport: HostTransport;
  /** Optional, for tests: the window's controls in place of Tauri's, which the page makes itself inside the desktop
   * app. */
  windowControls?: WindowControls | null | undefined;
  /** Optional, for the review page: the section to open at, in place of the address's hash. */
  section?: SectionId | undefined;
}) {
  const w = useSettingsWords();
  const display = useDisplay();
  const settings = useSettings(store);
  // the window's own limits store, over its channel's limits_view_changed: Plan and usage reads it
  const [limits] = useState(() => limitsStoreOf(transport));
  const ctx = contextOf(store, transport, settings, display);
  const shown = shownSections(ctx);
  const [chosen, setChosen] = useState<SectionId | undefined>(() =>
    sectionOf(section === undefined ? (globalThis.location?.hash ?? "") : `#${section}`, shown),
  );
  // a section that stops showing gives way to the first
  const open = shown.find((s) => s.id === chosen) ?? shown[0];
  // the window's own controls, inside the desktop app only
  const [controls] = useState(() =>
    windowControls !== undefined
      ? windowControls
      : transport.kind === "tauri"
        ? tauriWindowControls()
        : null,
  );
  // the caption buttons' commands, off the Mac: its own lights need none
  const wired = useMemo(
    () => (display.platform === "mac" ? null : controls),
    [display.platform, controls],
  );
  const caption = useWindowFrame(wired);
  // the window names itself in its language: the page's title for assistive technology, and the native title Windows
  // shows in the taskbar and Alt-Tab
  const title = w.ui("Settings");
  useEffect(() => {
    document.title = title;
    setWindowTitle(title);
  }, [title]);
  const onSection = (id: string) => {
    const next = shown.find((s) => s.id === id);
    if (next === undefined) return;
    setChosen(next.id);
    // the address keeps the section, so a reload opens it again
    history.replaceState(history.state, "", `#${next.id}`);
  };
  return (
    <PartsRoot>
      <LimitsContext value={limits}>
        <SettingsWindow
          platform={display.platform}
          title={title}
          sections={shown.map((s) => ({
            id: s.id,
            name: w.s(s.name),
            ...(s.note === undefined ? {} : { note: s.note(ctx, w.lang) }),
          }))}
          current={open?.id ?? ""}
          onSection={onSection}
          lead={open?.lead === undefined ? undefined : w.s(open.lead)}
          caption={caption}
          nativeLights={controls !== null}
        >
          {open !== undefined && <open.Body ctx={ctx} />}
        </SettingsWindow>
      </LimitsContext>
    </PartsRoot>
  );
}
