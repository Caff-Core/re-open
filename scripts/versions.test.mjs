import { describe, expect, it } from 'vitest';
import { readVersions, workspaceVersion } from './versions.mjs';

const root = new URL('../', import.meta.url);

describe('workspaceVersion', () => {
  it('reads the version from [workspace.package] only', () => {
    const toml = '[package]\nversion = "9.9.9"\n\n[workspace.package]\nversion = "1.2.3"\n\n[x]\n';
    expect(workspaceVersion(toml)).toBe('1.2.3');
  });
});

describe('repository versions', () => {
  const { cargo, manifests, tauriOverride } = readVersions(root);

  it('has a semver workspace version', () => {
    expect(cargo).toMatch(/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/);
  });

  it('keeps shipped package manifests on the workspace version', () => {
    for (const [path, version] of Object.entries(manifests)) {
      expect({ path, version }).toEqual({ path, version: cargo });
    }
  });

  it('does not override the version in tauri.conf.json', () => {
    expect(tauriOverride).toBeUndefined();
  });
});
