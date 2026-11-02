// Every app test file's setup (vite.config.ts): a test ends only once the room's late imports it started have landed
// (room/late.ts), so none is still loading, or warns that it failed, after the file's environment is torn down.
import { afterEach } from "vitest";
import { settled } from "../room/late.ts";

afterEach(settled);
