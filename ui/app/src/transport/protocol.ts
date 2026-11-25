// The protocol's types as the app reads them, from the bindings tauri-specta generates (bindings.ts, the one TS home of
// those types). The store and the window import types from here, never from bindings.ts, so nothing outside
// src/transport/ reaches the module that imports Tauri. Types only; nothing here runs.
export type * from "./bindings.ts";
