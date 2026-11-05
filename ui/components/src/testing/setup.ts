// Every test file's setup, in both vitest projects (ui/app/vite.config.ts): a test ends only once the late imports it
// started have landed (lib/late.ts), so none is still loading, or logs, after the file's environment is torn down.
// They land inside act: a lazy part that suspended is then React's to finish, and React has nothing to warn about.
import { act } from "react";
import { afterEach } from "vitest";
import { pending, settled } from "../lib/late.ts";
// the Hindi words, which the app loads as a lazy chunk for a Hindi display, are in from the start in every test, so a
// test can render in Hindi at once; display-hindi.test.tsx empties them to test the load itself
import "../lib/words-hi.ts";

afterEach(async () => {
  // only a test that rendered something lazy has a load on its way, and such a test is in React's act environment
  if (pending()) await act(settled);
});
