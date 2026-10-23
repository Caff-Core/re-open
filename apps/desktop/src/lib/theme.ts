import { useEffect, useState } from 'react';

export type ThemePreference = 'system' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';

const DARK_QUERY = '(prefers-color-scheme: dark)';

export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  if (preference === 'system') return systemPrefersDark ? 'dark' : 'light';
  return preference;
}

export function applyTheme(root: HTMLElement, theme: ResolvedTheme): void {
  root.classList.toggle('dark', theme === 'dark');
  root.style.colorScheme = theme;
}

/**
 * Follows the desktop's light/dark preference unless overridden.
 * The override is session-only; persisted settings go through the agent (Phase 4).
 */
export function useTheme(): [ThemePreference, (preference: ThemePreference) => void] {
  const [preference, setPreference] = useState<ThemePreference>('system');

  useEffect(() => {
    const media = window.matchMedia(DARK_QUERY);
    const update = () => {
      applyTheme(document.documentElement, resolveTheme(preference, media.matches));
    };
    update();
    if (preference !== 'system') return undefined;
    media.addEventListener('change', update);
    return () => {
      media.removeEventListener('change', update);
    };
  }, [preference]);

  return [preference, setPreference];
}
