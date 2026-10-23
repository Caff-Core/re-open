// Application version consistency. The Cargo workspace version is the single
// source for the desktop app (tauri.conf.json inherits it); JavaScript package
// manifests that ship with the app must match it. See ADR-015.
import { readFileSync } from 'node:fs';

/** @param {URL} url */
const readJson = (url) => JSON.parse(readFileSync(url, 'utf8'));

/**
 * @param {string} cargoToml
 * @returns {string | undefined}
 */
export function workspaceVersion(cargoToml) {
  const section = cargoToml.split(/^\[workspace\.package\]$/m)[1]?.split(/^\[/m)[0];
  return section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
}

/**
 * Shipped JavaScript manifests that must carry the app version.
 * Add the browser extension manifest here in task 3.4.
 */
export const versionedManifests = ['apps/desktop/package.json'];

/** @param {URL} root */
export function readVersions(root) {
  const cargo = workspaceVersion(readFileSync(new URL('Cargo.toml', root), 'utf8'));
  const manifests = Object.fromEntries(
    versionedManifests.map((path) => [path, readJson(new URL(path, root)).version]),
  );
  const tauriConfig = readJson(new URL('apps/desktop/src-tauri/tauri.conf.json', root));
  return { cargo, manifests, tauriOverride: tauriConfig.version };
}
