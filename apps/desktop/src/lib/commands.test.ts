import { clearMocks, mockIPC } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it } from 'vitest';
import { appVersion } from './commands';

afterEach(() => {
  clearMocks();
});

describe('appVersion', () => {
  it('invokes the app_version command without arguments', async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return '0.1.0';
    });

    await expect(appVersion()).resolves.toBe('0.1.0');
    expect(calls).toEqual([{ cmd: 'app_version', args: {} }]);
  });

  it('propagates command errors', async () => {
    mockIPC(() => {
      throw new Error('unavailable');
    });

    await expect(appVersion()).rejects.toThrow('unavailable');
  });
});
