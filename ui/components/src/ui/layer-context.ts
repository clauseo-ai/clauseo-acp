import { createContext, useContext } from "react";

/** Where raised planes mount: an element inside the window's main landmark, so a menu's rows are never stranded
 * outside every landmark. Without a LayerProvider they mount at the end of the body, as Base UI does. */
export const LayerContext = createContext<HTMLElement | null>(null);

/** The layer to mount a raised plane in, for a Base UI Portal's container. */
export function useLayer(): HTMLElement | undefined {
  return useContext(LayerContext) ?? undefined;
}
