// The version About shows: tauri.conf.json's (its one home), plus release.yml's CLAUSEO_VERSION_SUFFIX when it sets
// one, a dry run's `dev.<short sha>`, so a build made for testing never reads as the release it isn't
// (0.0.0-dev.1a2b3c4d5). Tags (release and test) set none: their version is the tag's. Only the display: the updater
// and the user agent read the plain version from Cargo.toml. vite.config.ts compiles the result in (version.ts).

/** `version`, with `-suffix` after it when `suffix` is set; a suffix that isn't a semver pre-release throws. */
export function appVersion(version: string, suffix: string | undefined): string {
  if (suffix === undefined || suffix === "") return version;
  if (!/^[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*$/.test(suffix)) {
    throw new Error(`CLAUSEO_VERSION_SUFFIX ${JSON.stringify(suffix)} is not a semver pre-release`);
  }
  return `${version}-${suffix}`;
}
