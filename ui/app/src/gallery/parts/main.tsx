// The dev-only gallery of Clauseo's parts: parts.html loads it, and Vite's build never does. A query such as
// ?theme=night&lang=hi&lite=1 sets the first look, for screenshots.
import "../gallery.css";
import "./parts.css";
import { startDisplay } from "@clauseo/components/lib/display.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { displayFromQuery } from "../query.ts";
import { PartsPage } from "./page.tsx";

startDisplay(displayFromQuery(globalThis.location.search));

const container = document.getElementById("root");
if (container === null) throw new Error("parts.html has no #root element");

createRoot(container).render(
  <StrictMode>
    <PartsRoot>
      <PartsPage />
    </PartsRoot>
  </StrictMode>,
);
