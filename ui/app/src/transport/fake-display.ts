// What FakeTransport's display_info reports, as a test or the review page scripts it: the OS, the release, and
// Windows' Smart App Control. Apart from fake-transport.ts to keep that file under the size limit.
import type { DisplayInfo } from "./host-transport.ts";

/** FakeHost's display_info part. */
export type FakeDisplayHost = {
  /** The OS display_info reports; Linux when left out. */
  platform?: DisplayInfo["platform"];
  /** The release date display_info reports; null, the dev and CI build's, when left out. */
  releaseDate?: DisplayInfo["release_date"];
  /** Whether display_info says this is a test build; false, a release build's, when left out. */
  testBuild?: boolean;
  /** The release mode display_info reports: `test` in a test build, `release` otherwise, when left out. */
  releaseMode?: DisplayInfo["release_mode"];
  /** Windows' Smart App Control state display_info reports; null, unknown (macOS, Linux, unreadable), when left out. */
  smartAppControl?: DisplayInfo["smart_app_control"];
};

/** display_info's answer for `host`. */
export const displayInfoOf = (host: FakeDisplayHost): DisplayInfo => ({
  platform: host.platform ?? "linux",
  release_date: host.releaseDate ?? null,
  test_build: host.testBuild ?? false,
  release_mode: host.releaseMode ?? (host.testBuild ? "test" : "release"),
  smart_app_control: host.smartAppControl ?? null,
});
