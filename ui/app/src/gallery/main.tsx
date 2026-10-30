// The dev-only gallery of the design system's controls: gallery.html loads it, and Vite's build never does. A query
// such as ?theme=night&lang=hi&lite=1 sets the first look, for screenshots.
import "./gallery.css";
import { startDisplay } from "@clauseo/components/lib/display.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Gallery } from "./gallery.tsx";
import { displayFromQuery } from "./query.ts";

startDisplay(displayFromQuery(globalThis.location.search));

const container = document.getElementById("root");
if (container === null) throw new Error("gallery.html has no #root element");

createRoot(container).render(
  <StrictMode>
    <PartsRoot>
      <Gallery />
    </PartsRoot>
  </StrictMode>,
);
