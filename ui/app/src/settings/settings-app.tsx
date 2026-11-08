// The settings window's page: the frame (ui/components' SettingsWindow) over the sections built so far (sections.tsx),
// each control one patch to the display store, which both windows share. The open section follows the address's hash
// (use-hash-section.ts). Each time the window opens it sends the connectors' `check` once, since the host reads the
// connector's liveness only then and the Connectors section shows only once a state lists it (U15). The store says when the settings or the
// locks change, from this window, the other, or the host, and the controls follow; the window's own caption buttons
// are wired off the Mac, as the room's are.
import { SettingsWindow } from "@clauseo/components/clauseo/settings-window.tsx";
import { useDisplay } from "@clauseo/components/lib/display.ts";
import { useSettingsWords } from "@clauseo/components/lib/words-settings.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { DisplayStore } from "../room/display-store.ts";
import { connectorsStoreOf, useConnectors, useConnectorsHeard } from "../store/connectors.ts";
import { LimitsContext, limitsStoreOf } from "../store/limits.ts";
import type { HostTransport } from "../transport/host-transport.ts";
import { tauriWindowControls, type WindowControls } from "../transport/window-controls.ts";
import { setWindowTitle } from "../transport/window-title.ts";
import { useWindowFrame } from "../window/use-window-frame.ts";
import { sendConnector } from "./connector-commands.ts";
import { contextOf, shownSections, type SectionEntry, type SectionId } from "./sections.tsx";
import { useHashSection } from "./use-hash-section.ts";
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
  const limitsView = useSyncExternalStore(limits.subscribe, limits.view, limits.view);
  // the release date the host reports, which the store hears as it arrives
  const subscribeRelease = useCallback(
    (listener: () => void) => store.subscribe(listener),
    [store],
  );
  const readRelease = useCallback(() => store.releaseDate(), [store]);
  const releaseDate = useSyncExternalStore(subscribeRelease, readRelease, readRelease);
  // the connectors' state, over the window's app feed; check once per open (the window is made on open and destroyed
  // on close), before any state, whichever section opens; the ref holds through StrictMode's second run
  const [connectors] = useState(() => connectorsStoreOf(transport));
  const connectorsState = useConnectors(connectors);
  const connectorsHeard = useConnectorsHeard(connectors);
  const checked = useRef(false);
  useEffect(() => {
    if (checked.current) return;
    checked.current = true;
    sendConnector(transport, { check: {} }).catch(() => {});
  }, [transport]);
  const ctx = contextOf(
    store,
    transport,
    settings,
    display,
    limitsView,
    releaseDate,
    connectorsState,
  );
  const shown = shownSections(ctx);
  // a section that stops showing gives way to the first; a hash's section that isn't shown yet waits for the first
  // connectors state
  const follow = useHashSection(
    shown.map((s) => s.id),
    connectorsHeard,
    section,
  );
  const open = shown.find((s) => s.id === follow.open);
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
  const words = (key: NonNullable<SectionEntry["lead"]>) =>
    typeof key === "function" ? key(w.lang) : w.s(key);
  return (
    <PartsRoot>
      <LimitsContext value={limits}>
        <SettingsWindow
          platform={display.platform}
          title={title}
          sections={shown.map((s) => {
            const note = s.note?.(ctx, w.lang);
            const Glyph = s.icon;
            return {
              id: s.id,
              name: words(s.name),
              icon: <Glyph size={16} />,
              ...(note === undefined ? {} : { note }),
            };
          })}
          current={open?.id ?? ""}
          onSection={follow.choose}
          focusHead={follow.focusHead}
          lead={open?.lead === undefined ? undefined : words(open.lead)}
          caption={caption}
          nativeLights={controls !== null}
        >
          {open !== undefined && <open.Body ctx={ctx} />}
        </SettingsWindow>
      </LimitsContext>
    </PartsRoot>
  );
}
