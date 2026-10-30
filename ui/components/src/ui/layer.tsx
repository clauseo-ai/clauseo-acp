import { useState, type ReactNode } from "react";
import { LayerContext } from "./layer-context.ts";

/** Gives the menus and selects inside it a layer to mount in, at the end of what it wraps. Wrap a page's main
 * landmark's content in it, so an open menu stays inside the landmark. Dialogs and toasts don't need it: they are
 * their own regions. */
export function LayerProvider({ children }: { children: ReactNode }) {
  const [layer, setLayer] = useState<HTMLElement | null>(null);
  return (
    <LayerContext value={layer}>
      {children}
      <div ref={setLayer} data-layer="" className="contents" />
    </LayerContext>
  );
}
