// The app's icons are imported as URLs, as the bundler serves them. This declaration serves ui/components' own
// tsconfig, which sets "types": [] and so has no vite/client. ui/app gets "*.svg" from vite/client instead; this file
// is not a duplicate of that one, and type-checking the package on its own needs it.
declare module "*.svg" {
  const url: string;
  export default url;
}
