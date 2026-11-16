// When the room shows the Smart App Control note: the host says Smart App Control is in evaluation (display_info's
// smart_app_control, read once per run on Windows; null on macOS and Linux, or unreadable) and the lawyer hasn't
// pressed "Got it" before (kept.ts). Asked once per room; nothing waits on the answer, and a failed ask shows nothing.
// On, Off and unknown show nothing: Clauseo is unsigned, so while it runs Smart App Control is Off or still deciding,
// and only "still deciding" can later start blocking it.
import { useEffect, useState } from "react";
import { hostDisplayInfo } from "../../platform.ts";
import type { HostTransport } from "../../transport/host-transport.ts";
import { markSacNoted, sacNoted } from "./kept.ts";

export function useSacNote(transport: HostTransport): { show: boolean; dismiss: () => void } {
  const [show, setShow] = useState(false);
  useEffect(() => {
    if (sacNoted()) return;
    let live = true;
    hostDisplayInfo(transport).then(
      (info) => {
        if (live && info.smart_app_control === "evaluation" && !sacNoted()) setShow(true);
      },
      () => {},
    );
    return () => {
      live = false;
    };
  }, [transport]);
  const dismiss = () => {
    markSacNoted();
    setShow(false);
  };
  return { show, dismiss };
}
