import { CSPProvider } from "@base-ui/react/csp-provider";
import type { ReactNode } from "react";
import { TooltipProvider } from "./tooltip.tsx";

/** Wraps a page's root once. The CSPProvider turns off every <style> element Base UI would add, which the app's CSP
 * (style-src 'self') refuses; the TooltipProvider times every tooltip on the page together. */
export function PartsRoot({ children }: { children: ReactNode }) {
  return (
    <CSPProvider disableStyleElements>
      <TooltipProvider>{children}</TooltipProvider>
    </CSPProvider>
  );
}
