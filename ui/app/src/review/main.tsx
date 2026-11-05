// The dev-only copy review: review.html loads it, `pnpm build:review` builds it into dist-review/, and the app's
// build never does. Without a query it is the list of strings; with ?cell=<id>&lang=hi&theme=night it is one
// registered state in one language and light, for the list's frames: display.ts sets one display per document.
import "./review.css";
import { startDisplay, whenWordsReady } from "@clauseo/components/lib/display.ts";
import { PartsRoot } from "@clauseo/components/ui/root.tsx";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { displayFromQuery } from "../gallery/query.ts";
import { Cell } from "./cell.tsx";
import { ReviewList } from "./list.tsx";

const query = globalThis.location.search;
const cell = new URLSearchParams(query).get("cell");
const display = displayFromQuery(query);
startDisplay(display);

const container = document.getElementById("root");
if (container === null) throw new Error("review.html has no #root element");

whenWordsReady(() =>
  createRoot(container).render(
    <StrictMode>
      <PartsRoot>
        {cell === null ? <ReviewList /> : <Cell id={cell} lang={display.lang} />}
      </PartsRoot>
    </StrictMode>,
  ),
);
