import { Button } from '@workspace/ui/components/button';
import { Badge, type BadgeProps } from '@workspace/ui/components/reui/badge';
import { useEffect, useState } from 'react';
import { appVersion } from './lib/commands';
import { type ThemePreference, useTheme } from './lib/theme';

type VersionState =
  { status: 'loading' } | { status: 'ready'; version: string } | { status: 'error' };

// Bootstrap smoke content for the design system; real status mapping arrives in Phase 4.
const SAMPLE_BADGES: { label: string; variant: BadgeProps['variant'] }[] = [
  { label: 'Complete', variant: 'success-light' },
  { label: 'Partial', variant: 'warning-light' },
  { label: 'Unavailable', variant: 'destructive-light' },
  { label: 'Stale', variant: 'info-light' },
  { label: 'Unsupported', variant: 'invert-light' },
];

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

export function App() {
  const [version, setVersion] = useState<VersionState>({ status: 'loading' });
  const [theme, setTheme] = useTheme();

  useEffect(() => {
    let cancelled = false;
    appVersion().then(
      (value) => {
        if (!cancelled) setVersion({ status: 'ready', version: value });
      },
      () => {
        if (!cancelled) setVersion({ status: 'error' });
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main className="flex min-h-svh flex-col gap-8 p-6">
      <header className="flex flex-col gap-0.5">
        <h1 className="text-xl font-semibold tracking-tight">Re-Open</h1>
        <p role="status" className="text-sm text-muted-foreground">
          {version.status === 'loading' && 'Loading version…'}
          {version.status === 'ready' && `Version ${version.version}`}
          {version.status === 'error' && 'Version unavailable'}
        </p>
      </header>

      <section aria-labelledby="badges-heading" className="flex flex-col gap-3">
        <h2 id="badges-heading" className="text-sm font-medium">
          Status badges
        </h2>
        <div className="flex flex-wrap gap-2">
          {SAMPLE_BADGES.map((badge) => (
            <Badge key={badge.label} variant={badge.variant}>
              {badge.label}
            </Badge>
          ))}
        </div>
      </section>

      <section aria-labelledby="theme-heading" className="flex flex-col gap-3">
        <h2 id="theme-heading" className="text-sm font-medium">
          Theme
        </h2>
        <div role="group" aria-labelledby="theme-heading" className="flex gap-2">
          {THEME_OPTIONS.map((option) => (
            <Button
              key={option.value}
              type="button"
              variant={theme === option.value ? 'default' : 'outline'}
              aria-pressed={theme === option.value}
              onClick={() => {
                setTheme(option.value);
              }}
            >
              {option.label}
            </Button>
          ))}
        </div>
      </section>
    </main>
  );
}
