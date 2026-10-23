// Typed wrappers for Tauri commands. Components call these, never `invoke` directly.
import { invoke } from '@tauri-apps/api/core';

export function appVersion(): Promise<string> {
  return invoke<string>('app_version');
}
