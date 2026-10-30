// The few Node APIs the tests in ui/components and ui/app call. The ui workspace installs no @types/node, so these
// declarations stand in for exactly what the tests use. ui/app's tsconfig includes ui/components/src, so they hold
// in both packages.
declare module "node:fs" {
  export function readFileSync(path: URL | string): Uint8Array;
  export function readFileSync(path: URL | string, encoding: "utf8"): string;
  export function readdirSync(path: URL | string): string[];
  export function readdirSync(path: URL | string, options: { recursive: true }): string[];
  export function realpathSync(path: URL | string): string;
  export function lstatSync(path: URL | string): { isSymbolicLink(): boolean };
}
declare module "node:crypto" {
  export function createHash(algorithm: "sha256"): {
    update(data: Uint8Array): { digest(encoding: "hex"): string };
  };
}
