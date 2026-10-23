import { describe, expect, it } from 'vitest';
import { applyTheme, resolveTheme } from './theme';

describe('resolveTheme', () => {
  it('follows the system preference by default', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
  });

  it('lets an explicit choice override the system', () => {
    expect(resolveTheme('light', true)).toBe('light');
    expect(resolveTheme('dark', false)).toBe('dark');
  });
});

describe('applyTheme', () => {
  it('toggles the dark class and color scheme', () => {
    const root = document.createElement('html');
    applyTheme(root, 'dark');
    expect(root.classList.contains('dark')).toBe(true);
    expect(root.style.colorScheme).toBe('dark');
    applyTheme(root, 'light');
    expect(root.classList.contains('dark')).toBe(false);
    expect(root.style.colorScheme).toBe('light');
  });
});
