// The app's icons are imported as URLs, as the bundler serves them.
declare module "*.svg" {
  const url: string;
  export default url;
}
