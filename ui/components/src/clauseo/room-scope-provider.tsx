import { useEffect, useId, useState, type ReactNode } from "react";
import { createScope, enterRoom, leaveRoom, RoomContext, markUsed } from "./room-scope.ts";

/** One room's scope, for everything inside it. Its timers and its hand stop when it leaves the page. */
export function RoomScopeProvider({ children }: { children: ReactNode }) {
  const prefix = useId();
  const [scope] = useState(() => createScope(prefix));
  useEffect(() => {
    enterRoom(scope);
    const win = scope.nodes.get("window");
    const mine = () => markUsed(scope);
    win?.addEventListener("pointerdown", mine);
    win?.addEventListener("focusin", mine);
    return () => {
      win?.removeEventListener("pointerdown", mine);
      win?.removeEventListener("focusin", mine);
      leaveRoom(scope);
      scope.hand.dispose();
      scope.motion.stopAll();
    };
  }, [scope]);
  return <RoomContext value={scope}>{children}</RoomContext>;
}
