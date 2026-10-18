import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { NullTransport } from "./transport/host-transport.ts";

const container = document.getElementById("root");
if (container === null) {
  throw new Error("index.html has no #root element");
}

createRoot(container).render(
  <StrictMode>
    <App transport={new NullTransport()} />
  </StrictMode>,
);
