// The app's version, as About shows it: crates/desktop/tauri.conf.json's `version` (its one home), compiled in by
// vite.config.ts's `define`, under vitest too.

declare const __CLAUSEO_VERSION__: string;

/** The version this build was made from, such as "3.0.4". */
export const APP_VERSION: string = __CLAUSEO_VERSION__;
