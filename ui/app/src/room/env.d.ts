// "1" in VITE_CLAUSEO_HAND_CHECKS builds in the hooks for checks by hand (start.ts, dev-hooks.ts): the e2e and probe
// builds.
interface ImportMetaEnv {
  readonly VITE_CLAUSEO_HAND_CHECKS?: string;
}
