import { useEffect, useId, useState, type ReactNode } from "react";
import { createScope, enterRoom, leaveRoom, RoomContext, markUsed } from "./room-scope.ts";

/** One room's scope, and the room's window: the element that holds everything in the room. The provider renders and
 * registers it as the scope's "window", so keys, presses and focus find their room without the screen wiring it,
 * and a key pressed in one room's composer is never another room's. Its timers and its hand stop when it leaves the
 * page. */
export function RoomScopeProvider({
  children,
  className,
}: {
  children: ReactNode;
  /** The window's classes, from the screen that lays the room out. */
  className?: string;
}) {
  const prefix = useId();
  const [{ scope, windowRef }] = useState(() => {
    const created = createScope(prefix);
    return { scope: created, windowRef: created.nodes.slot<HTMLDivElement>("window") };
  });
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
  return (
    <RoomContext value={scope}>
      <div ref={windowRef} className={className}>
        {children}
      </div>
    </RoomContext>
  );
}
